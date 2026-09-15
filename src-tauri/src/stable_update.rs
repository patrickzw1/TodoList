use reqwest::{Client, StatusCode};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tauri::{Manager, ResourceId, Runtime, Url, Webview};
use tauri_plugin_updater::UpdaterExt;

const RELEASES_API: &str = "https://api.github.com/repos/patrickzw1/TodoList/releases";
const RELEASE_DOWNLOADS: &str = "https://github.com/patrickzw1/TodoList/releases/download/";
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const PAGE_SIZE: usize = 100;
const MAX_RELEASE_PAGES: usize = 100;

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    state: String,
    size: u64,
}

#[derive(Debug)]
struct StableRelease {
    tag: String,
    version: Version,
    assets: Vec<ReleaseAsset>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StableUpdateMetadata {
    rid: ResourceId,
    current_version: String,
    version: String,
    body: Option<String>,
    raw_json: serde_json::Value,
}

fn remaining_timeout(started: Instant) -> Result<Duration, String> {
    CHECK_TIMEOUT
        .checked_sub(started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| "版本检查失败：检查更新超时，请稍后重试。".into())
}

fn stable_version(tag: &str) -> Option<Version> {
    let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
    (version.pre.is_empty() && version.build.is_empty()).then_some(version)
}

fn select_highest(highest: &mut Option<StableRelease>, releases: Vec<GithubRelease>) {
    for release in releases {
        if release.draft || release.prerelease {
            continue;
        }
        let Some(version) = stable_version(&release.tag_name) else {
            continue;
        };
        // Compare SemVer numerically across every page; GitHub ordering and Latest are irrelevant.
        // Prefer the canonical v-prefixed tag if both tag spellings describe the same version.
        if highest.as_ref().is_none_or(|current| {
            version > current.version
                || (version == current.version && release.tag_name > current.tag)
        }) {
            *highest = Some(StableRelease {
                tag: release.tag_name,
                version,
                assets: release.assets,
            });
        }
    }
}

fn http_failure(status: StatusCode, rate_limited: bool) -> String {
    if rate_limited || status == StatusCode::TOO_MANY_REQUESTS {
        "版本检查失败：GitHub 请求已限流，请稍后重试。".into()
    } else {
        format!("版本检查失败：GitHub Release 服务返回 HTTP {status}，请稍后重试。")
    }
}

async fn highest_stable_release(
    client: &Client,
    api_url: &str,
    started: Instant,
) -> Result<StableRelease, String> {
    let mut highest = None;
    let mut page = 1;
    loop {
        let mut page_url =
            Url::parse(api_url).map_err(|_| "版本检查失败：GitHub Release 地址无效。")?;
        page_url
            .query_pairs_mut()
            .append_pair("per_page", &PAGE_SIZE.to_string())
            .append_pair("page", &page.to_string());
        let response = client
            .get(page_url)
            .timeout(remaining_timeout(started)?)
            .send()
            .await
            .map_err(|_| "版本检查失败：无法连接 GitHub Release 服务或请求超时。")?;
        if !response.status().is_success() {
            let rate_limited = response.status() == StatusCode::FORBIDDEN
                && (response
                    .headers()
                    .get("x-ratelimit-remaining")
                    .is_some_and(|v| v == "0")
                    || response.headers().contains_key("retry-after"));
            return Err(http_failure(response.status(), rate_limited));
        }
        let releases: Vec<GithubRelease> = response
            .json()
            .await
            .map_err(|_| "版本检查失败：GitHub Release 列表无法解析，不能确认最新版本。")?;
        let count = releases.len();
        select_highest(&mut highest, releases);
        // A full last page is followed by an empty page, so no stable release is skipped.
        if count < PAGE_SIZE {
            break;
        }
        if page >= MAX_RELEASE_PAGES {
            return Err(
                "版本检查失败：GitHub Release 页数超出检查上限，不能确认最高正式版本。".into(),
            );
        }
        page += 1;
    }
    highest.ok_or_else(|| "版本检查失败：未找到有效的公开正式版本，不能确认最新版本。".into())
}

fn needs_update(release: &StableRelease, installed: &Version) -> bool {
    release.version > *installed
}

fn asset_url(release: &StableRelease, asset: &ReleaseAsset) -> Result<Url, String> {
    let failure = || {
        format!(
            "版本检查失败：最高正式版本 {} 的更新资源缺失或来源不匹配。",
            release.version
        )
    };
    if asset.name.is_empty()
        || asset.name.contains(['/', '\\'])
        || asset.state != "uploaded"
        || asset.size == 0
    {
        return Err(failure());
    }
    let mut expected = Url::parse(RELEASE_DOWNLOADS).map_err(|_| failure())?;
    expected
        .path_segments_mut()
        .map_err(|_| failure())?
        .pop_if_empty()
        .push(&release.tag)
        .push(&asset.name);
    let actual = Url::parse(&asset.browser_download_url).map_err(|_| failure())?;
    if actual != expected {
        return Err(failure());
    }
    Ok(expected)
}

fn manifest_endpoint(release: &StableRelease) -> Result<Url, String> {
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == "latest.json")
        .ok_or_else(|| {
            format!(
                "版本检查失败：最高正式版本 {} 缺少 latest.json 更新清单。",
                release.version
            )
        })?;
    asset_url(release, asset)
}

fn validate_update(
    release: &StableRelease,
    version: &str,
    download_url: &Url,
    signature: &str,
) -> Result<(), String> {
    if version != release.version.to_string() {
        return Err(format!(
            "版本检查失败：最高正式版本 {} 的更新清单版本不匹配。",
            release.version
        ));
    }
    if signature.trim().is_empty() {
        return Err(format!(
            "版本检查失败：最高正式版本 {} 的更新包缺少签名。",
            release.version
        ));
    }
    let owned_package = release.assets.iter().any(|asset| {
        asset.name != "latest.json"
            && !asset.name.ends_with(".sig")
            && asset_url(release, asset).is_ok_and(|url| url == *download_url)
    });
    if !owned_package {
        return Err(format!(
            "版本检查失败：最高正式版本 {} 的更新包缺失或来源不匹配。",
            release.version
        ));
    }
    Ok(())
}

fn require_production_channel(production: bool, identifier: &str) -> Result<(), String> {
    if !production || identifier != "app.todolist.desktop" {
        return Err(
            "当前构建未配置更新源；开发版不会访问生产更新服务，请在日常安装版检查更新。".into(),
        );
    }
    Ok(())
}

fn initialize_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

#[tauri::command]
pub(crate) async fn check_stable_update<R: Runtime>(
    webview: Webview<R>,
) -> Result<Option<StableUpdateMetadata>, String> {
    // Reject development and acceptance builds before creating a client or making any request.
    require_production_channel(cfg!(feature = "production"), &webview.config().identifier)?;
    let started = Instant::now();
    initialize_crypto_provider();
    let client = Client::builder()
        .user_agent(concat!("TodoList/", env!("CARGO_PKG_VERSION")))
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::ACCEPT,
                "application/vnd.github+json".parse().unwrap(),
            );
            headers.insert("x-github-api-version", "2022-11-28".parse().unwrap());
            headers
        })
        .timeout(CHECK_TIMEOUT)
        .build()
        .map_err(|_| "版本检查失败：无法初始化安全更新连接。")?;
    let release = highest_stable_release(&client, RELEASES_API, started).await?;
    if !needs_update(&release, &webview.package_info().version) {
        return Ok(None);
    }
    let endpoint = manifest_endpoint(&release)?;
    let update = webview.updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|_| "版本检查失败：无法配置最高正式版本的更新清单地址。")?
        .timeout(remaining_timeout(started)?)
        .build()
        .map_err(|_| "版本检查失败：无法初始化签名更新检查。")?
        .check()
        .await
        .map_err(|_| format!("版本检查失败：最高正式版本 {} 的更新清单无法读取或当前平台更新资源缺失，请稍后重试。", release.version))?
        .ok_or_else(|| format!("版本检查失败：最高正式版本 {} 的更新清单版本不匹配，不能确认最新版本。", release.version))?;
    // Validate the metadata actually consumed by Tauri, including after redirects or a changed manifest.
    validate_update(
        &release,
        &update.version,
        &update.download_url,
        &update.signature,
    )?;
    let metadata = StableUpdateMetadata {
        current_version: update.current_version.clone(),
        version: update.version.clone(),
        body: update.body.clone(),
        raw_json: update.raw_json.clone(),
        // Reuse the plugin resource type; download still verifies the configured Minisign key
        // and installation still retains its existing installer hooks and explicit user action.
        rid: webview.resources_table().add(update),
    };
    Ok(Some(metadata))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn github_release(tag: &str, draft: bool, prerelease: bool) -> serde_json::Value {
        json!({"tag_name": tag, "draft": draft, "prerelease": prerelease, "assets": []})
    }

    fn release(version: &str) -> StableRelease {
        let tag = format!("v{version}");
        StableRelease {
            version: Version::parse(version).unwrap(),
            assets: ["latest.json", "TodoList_setup.exe"]
                .map(|name| ReleaseAsset {
                    name: name.into(),
                    browser_download_url: format!("{RELEASE_DOWNLOADS}{tag}/{name}"),
                    state: "uploaded".into(),
                    size: 123,
                })
                .into(),
            tag,
        }
    }

    // Exercise real reqwest pagination/status/body handling against an owned loopback fixture.
    fn api_fixture(
        responses: Vec<(u16, &str, serde_json::Value)>,
    ) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let responses: Vec<_> = responses
            .into_iter()
            .map(|(status, headers, body)| (status, headers.to_owned(), body.to_string()))
            .collect();
        let handle = thread::spawn(move || {
            let mut paths = Vec::new();
            for (status, headers, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                loop {
                    let count = stream.read(&mut buffer).unwrap();
                    request.extend_from_slice(&buffer[..count]);
                    if count == 0 || request.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                paths.push(
                    String::from_utf8(request)
                        .unwrap()
                        .lines()
                        .next()
                        .unwrap()
                        .to_owned(),
                );
                write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len()).unwrap();
            }
            paths
        });
        (format!("http://{address}/releases"), handle)
    }

    fn fetch(
        responses: Vec<(u16, &str, serde_json::Value)>,
    ) -> (Result<StableRelease, String>, Vec<String>) {
        let (url, server) = api_fixture(responses);
        initialize_crypto_provider();
        let client = Client::builder().no_proxy().build().unwrap();
        let result =
            tauri::async_runtime::block_on(highest_stable_release(&client, &url, Instant::now()));
        (result, server.join().unwrap())
    }

    #[test]
    fn late_publication_of_older_release_does_not_change_highest_version() {
        for releases in [
            vec![
                github_release("v0.2.9", false, false),
                github_release("v0.2.10", false, false),
            ],
            vec![
                github_release("v0.2.10", false, false),
                github_release("v0.2.9", false, false),
            ],
        ] {
            let (result, paths) = fetch(vec![(200, "", json!(releases))]);
            assert_eq!(result.unwrap().version, Version::new(0, 2, 10));
            assert_eq!(paths, ["GET /releases?per_page=100&page=1 HTTP/1.1"]);
        }
        assert!(stable_version("v0.10.0") > stable_version("v0.9.99"));
        assert!(stable_version("v1.0.0") > stable_version("v0.99.99"));
    }

    #[test]
    fn all_pages_are_compared_even_if_a_lower_version_appears_first() {
        let mut first = vec![github_release("v0.2.9", false, false); PAGE_SIZE];
        first[0] = github_release("v9.0.0", false, true);
        let (result, paths) = fetch(vec![
            (200, "", json!(first)),
            (200, "", json!([github_release("v0.2.10", false, false)])),
        ]);
        assert_eq!(result.unwrap().version, Version::new(0, 2, 10));
        assert_eq!(paths.len(), 2);
        assert!(paths[1].contains("page=2"));
        let (result, paths) = fetch(vec![
            (
                200,
                "",
                json!(vec![github_release("v0.2.10", false, false); PAGE_SIZE]),
            ),
            (200, "", json!([])),
        ]);
        assert_eq!(result.unwrap().version, Version::new(0, 2, 10));
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn drafts_prereleases_and_invalid_or_qualified_versions_are_ignored() {
        let (result, _) = fetch(vec![(
            200,
            "",
            json!([
                github_release("v99.0.0", true, false),
                github_release("v98.0.0", false, true),
                github_release("v97.0.0-beta.1", false, false),
                github_release("v96.0.0+build", false, false),
                github_release("v095.0.0", false, false),
                github_release("latest", false, false),
                github_release("v0.2.10", false, false),
            ]),
        )]);
        assert_eq!(result.unwrap().version, Version::new(0, 2, 10));
        assert_eq!(stable_version("0.2.10"), Some(Version::new(0, 2, 10)));
    }

    #[test]
    fn installed_equal_or_higher_versions_are_never_downgraded() {
        let highest = release("0.2.10");
        assert!(needs_update(&highest, &Version::new(0, 2, 9)));
        assert!(!needs_update(&highest, &Version::new(0, 2, 10)));
        assert!(!needs_update(&highest, &Version::new(0, 3, 0)));
    }

    #[test]
    fn highest_release_missing_resources_is_an_error_without_older_fallback() {
        let (result, _) = fetch(vec![(
            200,
            "",
            json!([
                github_release("v0.2.9", false, false),
                github_release("v0.2.10", false, false),
            ]),
        )]);
        let highest = result.unwrap();
        assert!(manifest_endpoint(&highest)
            .unwrap_err()
            .contains("0.2.10 缺少 latest.json"));
        let mut highest = release("0.2.10");
        let url = Url::parse(&highest.assets[1].browser_download_url).unwrap();
        assert!(validate_update(&highest, "0.2.10", &url, "signature").is_ok());
        highest.assets.pop();
        assert!(validate_update(&highest, "0.2.10", &url, "signature")
            .unwrap_err()
            .contains("更新包缺失"));
    }

    #[test]
    fn manifest_version_signature_and_release_owned_asset_sources_are_required() {
        let highest = release("0.2.10");
        let url = Url::parse(&highest.assets[1].browser_download_url).unwrap();
        assert_eq!(
            manifest_endpoint(&highest).unwrap().as_str(),
            format!("{RELEASE_DOWNLOADS}v0.2.10/latest.json")
        );
        assert!(validate_update(&highest, "0.2.9", &url, "signature")
            .unwrap_err()
            .contains("版本不匹配"));
        assert!(validate_update(&highest, "0.2.10", &url, " ")
            .unwrap_err()
            .contains("缺少签名"));
        for url in [
            format!("{RELEASE_DOWNLOADS}v0.2.9/TodoList_setup.exe"),
            "https://updates.invalid/TodoList_setup.exe".into(),
            format!("{RELEASE_DOWNLOADS}v0.2.10/missing.exe"),
            format!("{RELEASE_DOWNLOADS}v0.2.10/TodoList_setup.exe?redirect=old"),
        ] {
            assert!(
                validate_update(&highest, "0.2.10", &Url::parse(&url).unwrap(), "signature")
                    .is_err()
            );
        }
        for index in 0..2 {
            let mut highest = release("0.2.10");
            highest.assets[index].browser_download_url =
                "https://updates.invalid/latest.json".into();
            if index == 0 {
                assert!(manifest_endpoint(&highest).is_err());
            } else {
                assert!(validate_update(&highest, "0.2.10", &url, "signature").is_err());
            }
        }
    }

    #[test]
    fn incomplete_unuploaded_assets_are_not_update_sources() {
        for (state, size) in [("new", 123), ("uploaded", 0)] {
            let mut highest = release("0.2.10");
            highest.assets[0].state = state.into();
            highest.assets[0].size = size;
            assert!(manifest_endpoint(&highest).is_err());
        }
    }

    #[test]
    fn rate_limits_http_failures_invalid_json_and_empty_results_are_errors() {
        for (status, headers, body, expected) in [
            (
                403,
                "x-ratelimit-remaining: 0\r\n",
                json!({"message": "API rate limit exceeded"}),
                "限流",
            ),
            (429, "", json!({"message": "Too many requests"}), "限流"),
            (500, "", json!({"message": "Unavailable"}), "HTTP 500"),
            (200, "", json!({"unexpected": "object"}), "无法解析"),
            (200, "", json!([]), "未找到有效"),
        ] {
            let (result, _) = fetch(vec![(status, headers, body)]);
            assert!(result.unwrap_err().contains(expected));
        }
        let (result, _) = fetch(vec![
            (
                200,
                "",
                json!(vec![github_release("v0.2.9", false, false); PAGE_SIZE]),
            ),
            (
                403,
                "x-ratelimit-remaining: 0\r\n",
                json!({"message": "API rate limit exceeded"}),
            ),
        ]);
        assert!(result.unwrap_err().contains("限流"));
    }

    #[test]
    fn page_limit_does_not_return_a_partially_scanned_highest_version() {
        let pages = vec![
            (
                200,
                "",
                json!(vec![github_release("v0.2.9", false, false); PAGE_SIZE])
            );
            MAX_RELEASE_PAGES
        ];
        let (result, paths) = fetch(pages);
        assert_eq!(paths.len(), MAX_RELEASE_PAGES);
        assert!(result.unwrap_err().contains("不能确认最高正式版本"));
    }

    #[test]
    fn offline_and_exhausted_timeout_do_not_claim_latest() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        initialize_crypto_provider();
        let client = Client::builder().no_proxy().build().unwrap();
        assert!(tauri::async_runtime::block_on(highest_stable_release(
            &client,
            &format!("http://{address}/releases"),
            Instant::now()
        ))
        .is_err());
        assert!(remaining_timeout(Instant::now() - CHECK_TIMEOUT)
            .unwrap_err()
            .contains("超时"));
    }

    #[test]
    fn development_and_acceptance_channels_are_rejected_before_network_access() {
        assert!(require_production_channel(false, "app.todolist.desktop.dev").is_err());
        assert!(require_production_channel(false, "app.todolist.desktop").is_err());
        assert!(
            require_production_channel(true, "app.todolist.desktop.installer-acceptance").is_err()
        );
        assert!(require_production_channel(true, "app.todolist.desktop").is_ok());
    }
}
