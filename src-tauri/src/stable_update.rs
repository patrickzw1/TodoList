use reqwest::{header::HeaderMap, Client, StatusCode};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, ResourceId, Runtime, Url, Webview};
use tauri_plugin_updater::UpdaterExt;

const RELEASES_API: &str = "https://api.github.com/repos/patrickzw1/TodoList/releases";
const RELEASE_DOWNLOADS: &str = "https://github.com/patrickzw1/TodoList/releases/download/";
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const PAGE_SIZE: usize = 100;
const MAX_RELEASE_PAGES: usize = 100;
const AUTOMATIC_INTERVAL_MS: u64 = 6 * 60 * 60 * 1000;
const MANUAL_INTERVAL_MS: u64 = 60 * 1000;
const SECONDARY_BACKOFF_MS: u64 = 60 * 1000;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateCheckStatus {
    last_attempt_at: Option<u64>,
    last_checked_at: Option<u64>,
    next_automatic_check_at: Option<u64>,
    next_manual_check_at: Option<u64>,
    retry_at: Option<u64>,
    last_result: Option<String>,
    message: Option<String>,
    #[serde(default)]
    rate_limit_failures: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StableUpdateCheck {
    update: Option<StableUpdateMetadata>,
    status: UpdateCheckStatus,
    checked: bool,
}

fn wall_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn read_check_status(path: &Path) -> Result<UpdateCheckStatus, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UpdateCheckStatus::default());
        }
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= 16 * 1024 => {}
        _ => return Err("版本检查失败：本地更新检查记录不可用，未发送请求。".into()),
    }
    let content =
        fs::read(path).map_err(|_| "版本检查失败：无法读取本地更新检查记录，未发送请求。")?;
    serde_json::from_slice(&content)
        .map_err(|_| "版本检查失败：本地更新检查记录损坏，未发送请求。".into())
}

fn write_check_status(path: &Path, status: &UpdateCheckStatus) -> Result<(), String> {
    let save = || -> Result<(), Box<dyn std::error::Error>> {
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        file.write_all(&serde_json::to_vec(status)?)?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    };
    save().map_err(|_| "版本检查失败：无法保存本地更新检查记录，请检查目录权限。".into())
}

fn finish_check_status(status: &mut UpdateCheckStatus, cooldown: Option<u64>, now: u64) {
    // This timestamp labels the most recent successful check; failures keep
    // that evidence while last_attempt_at records the newer unsuccessful attempt.
    if matches!(status.last_result.as_deref(), Some("latest" | "available")) {
        status.last_checked_at = Some(now);
    }
    status.retry_at = cooldown;
    // There are no automatic retry loops. Repeated user retries after a true
    // rate-limit failure still back off exponentially, persisted across restarts.
    if status.last_result.as_deref() == Some("error") && cooldown.is_some() {
        let backoff = SECONDARY_BACKOFF_MS.saturating_mul(1 << status.rate_limit_failures.min(6));
        status.retry_at = Some(cooldown.unwrap().max(now.saturating_add(backoff)));
        status.rate_limit_failures = status.rate_limit_failures.saturating_add(1);
    } else {
        status.rate_limit_failures = 0;
    }
}

// A single native file lock covers every window and any second GUI process. The lock is
// released on crash, while the attempt saved before network I/O still survives restart.
fn admit_check(
    path: &Path,
    automatic: bool,
    now: u64,
) -> Result<(UpdateCheckStatus, Option<File>), String> {
    fs::create_dir_all(path.parent().unwrap())
        .map_err(|_| "版本检查失败：无法创建本地更新检查目录，未发送请求。")?;
    let lock_path = path.with_extension("lock");
    if fs::symlink_metadata(&lock_path)
        .is_ok_and(|metadata| !metadata.is_file() || metadata.file_type().is_symlink())
    {
        return Err("版本检查失败：本地更新检查锁不可用，未发送请求。".into());
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|_| "版本检查失败：无法打开本地更新检查锁，未发送请求。")?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Ok((read_check_status(path)?, None));
        }
        Err(_) => return Err("版本检查失败：无法锁定本地更新检查记录，未发送请求。".into()),
    }
    let mut status = read_check_status(path)?;
    let next = if automatic {
        status.next_automatic_check_at
    } else {
        status.next_manual_check_at
    };
    if next.is_some_and(|next| now < next) || status.retry_at.is_some_and(|retry| now < retry) {
        return Ok((status, None));
    }
    status.last_attempt_at = Some(now);
    status.next_automatic_check_at = Some(now.saturating_add(AUTOMATIC_INTERVAL_MS));
    status.next_manual_check_at = Some(now.saturating_add(MANUAL_INTERVAL_MS));
    status.retry_at = None;
    write_check_status(path, &status)?;
    Ok((status, Some(lock)))
}

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

// Retry-After permits an HTTP date as well as seconds. Parse the standard IMF-fixdate
// form directly so updating does not add a date library to the desktop process.
fn http_date_ms(value: &str) -> Option<u64> {
    let fields: Vec<_> = value.split_whitespace().collect();
    if fields.len() != 6 || fields[5] != "GMT" || !fields[0].ends_with(',') {
        return None;
    }
    let day: u64 = fields[1].parse().ok()?;
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|month| *month == fields[2])?;
    let year: u64 = fields[3].parse().ok()?;
    if !(1970..=9999).contains(&year) {
        return None;
    }
    let leap = |year: u64| year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let months = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if day == 0 || day > months[month] {
        return None;
    }
    let time: Vec<u64> = fields[4]
        .split(':')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if time.len() != 3 || time[0] > 23 || time[1] > 59 || time[2] > 59 {
        return None;
    }
    let days = (1970..year)
        .map(|year| if leap(year) { 366 } else { 365 })
        .sum::<u64>()
        + months[..month].iter().sum::<u64>()
        + day
        - 1;
    Some((days * 86_400 + time[0] * 3600 + time[1] * 60 + time[2]) * 1000)
}

fn header_number(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

fn rate_limit_retry_at(
    status: StatusCode,
    headers: &HeaderMap,
    message: &str,
    now: u64,
) -> Option<u64> {
    let exhausted = header_number(headers, "x-ratelimit-remaining") == Some(0);
    let limited_error = status == StatusCode::TOO_MANY_REQUESTS
        || (status == StatusCode::FORBIDDEN
            && (exhausted
                || headers.contains_key("retry-after")
                || message.to_ascii_lowercase().contains("rate limit")));
    if !limited_error && !(status.is_success() && exhausted) {
        return None;
    }
    let retry_after = headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .trim()
                .parse::<u64>()
                .ok()
                .map(|seconds| now.saturating_add(seconds.saturating_mul(1000)))
                .or_else(|| http_date_ms(value))
        });
    let reset = exhausted
        .then(|| header_number(headers, "x-ratelimit-reset"))
        .flatten()
        .map(|seconds| seconds.saturating_add(1).saturating_mul(1000));
    retry_after
        .into_iter()
        .chain(reset)
        .filter(|retry| *retry > now)
        .max()
        .or_else(|| Some(now.saturating_add(SECONDARY_BACKOFF_MS)))
}

async fn highest_stable_release(
    client: &Client,
    api_url: &str,
    started: Instant,
    clock: fn() -> u64,
    cooldown: &mut Option<u64>,
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
        let status = response.status();
        let headers = response.headers().clone();
        // Save header evidence before reading the body: a truncated error body
        // must not discard a server-provided reset or Retry-After interval.
        if let Some(retry_at) = rate_limit_retry_at(status, &headers, "", clock()) {
            *cooldown = Some(cooldown.unwrap_or(0).max(retry_at));
        }
        if !status.is_success() {
            let message = response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|body| body.get("message")?.as_str().map(str::to_owned))
                .unwrap_or_default();
            if let Some(retry_at) = rate_limit_retry_at(status, &headers, &message, clock()) {
                *cooldown = Some(cooldown.unwrap_or(0).max(retry_at));
            }
            return Err(http_failure(status, cooldown.is_some()));
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
        // The current page may consume the final request. A partial scan is never
        // treated as the highest version, and the next page waits for the server.
        if cooldown.is_some() {
            return Err(http_failure(StatusCode::TOO_MANY_REQUESTS, true));
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
    automatic: bool,
) -> Result<StableUpdateCheck, String> {
    // Reject development and acceptance builds before creating a client or making any request.
    require_production_channel(cfg!(feature = "production"), &webview.config().identifier)?;
    let path = webview
        .path()
        .app_local_data_dir()
        .map_err(|_| "版本检查失败：无法定位本地更新检查目录，未发送请求。")?
        .join("update-check-state.json");
    let (mut status, lock) = admit_check(&path, automatic, wall_time_ms())?;
    let Some(_lock) = lock else {
        return Ok(StableUpdateCheck {
            update: None,
            status,
            checked: false,
        });
    };
    let mut cooldown = None;
    let result = perform_update_check(&webview, &mut cooldown).await;
    let update = match result {
        Ok(update) => {
            status.last_result = Some(
                if update.is_some() {
                    "available"
                } else {
                    "latest"
                }
                .into(),
            );
            status.message = None;
            update
        }
        Err(message) => {
            status.last_result = Some("error".into());
            status.message = Some(message);
            None
        }
    };
    finish_check_status(&mut status, cooldown, wall_time_ms());
    // Keep the native lock until the final state is committed. A second window
    // observes the same attempt and cannot bypass either cooldown.
    if let Err(error) = write_check_status(&path, &status) {
        if let Some(update) = update {
            webview.resources_table().close(update.rid).ok();
        }
        return Err(error);
    }
    Ok(StableUpdateCheck {
        update,
        status,
        checked: true,
    })
}

async fn perform_update_check<R: Runtime>(
    webview: &Webview<R>,
    cooldown: &mut Option<u64>,
) -> Result<Option<StableUpdateMetadata>, String> {
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
    let release =
        highest_stable_release(&client, RELEASES_API, started, wall_time_ms, cooldown).await?;
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
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let responses: Vec<_> = responses
            .into_iter()
            .map(|(status, headers, body)| (status, headers.to_owned(), body.to_string()))
            .collect();
        let handle = thread::spawn(move || {
            let mut paths = Vec::new();
            for (status, headers, body) in responses {
                let started = Instant::now();
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(connection) => break connection,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && started.elapsed() < Duration::from_secs(5) =>
                        {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("loopback fixture received no connection: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
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
        let (result, paths, _) = fetch_with_cooldown(responses);
        (result, paths)
    }

    fn fetch_with_cooldown(
        responses: Vec<(u16, &str, serde_json::Value)>,
    ) -> (Result<StableRelease, String>, Vec<String>, Option<u64>) {
        let (url, server) = api_fixture(responses);
        initialize_crypto_provider();
        let client = Client::builder().no_proxy().build().unwrap();
        let mut cooldown = None;
        let result = tauri::async_runtime::block_on(highest_stable_release(
            &client,
            &url,
            Instant::now(),
            || 1_000,
            &mut cooldown,
        ));
        let paths = server
            .join()
            .unwrap_or_else(|_| panic!("loopback request failed: {result:?}"));
        (result, paths, cooldown)
    }

    #[test]
    fn native_lock_manual_interval_and_automatic_attempt_survive_reload() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("checks.json");
        let (status, lock) = admit_check(&path, true, 1_000).unwrap();
        assert!(lock.is_some());
        assert_eq!(
            read_check_status(&path).unwrap().last_attempt_at,
            Some(1_000)
        );
        // A second window/process cannot enter even if its clock passes every interval.
        assert!(admit_check(&path, false, AUTOMATIC_INTERVAL_MS * 2)
            .unwrap()
            .1
            .is_none());
        drop(lock);
        assert!(admit_check(&path, true, 1_001).unwrap().1.is_none());
        assert!(admit_check(&path, false, 60_999).unwrap().1.is_none());
        let (_, manual_lock) = admit_check(&path, false, 61_000).unwrap();
        assert!(manual_lock.is_some());
        drop(manual_lock);
        // The manual attempt also restarts the automatic interval.
        assert!(
            admit_check(&path, true, status.next_automatic_check_at.unwrap())
                .unwrap()
                .1
                .is_none()
        );
        assert!(admit_check(&path, true, 61_000 + AUTOMATIC_INTERVAL_MS)
            .unwrap()
            .1
            .is_some());
    }

    #[test]
    fn persisted_server_cooldown_blocks_both_check_modes_after_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("checks.json");
        let status = UpdateCheckStatus {
            last_attempt_at: Some(1_000),
            last_checked_at: Some(2_000),
            retry_at: Some(AUTOMATIC_INTERVAL_MS * 2),
            last_result: Some("error".into()),
            message: Some("GitHub 请求已限流".into()),
            ..UpdateCheckStatus::default()
        };
        write_check_status(&path, &status).unwrap();
        for automatic in [true, false] {
            let (loaded, lock) = admit_check(&path, automatic, AUTOMATIC_INTERVAL_MS).unwrap();
            assert!(lock.is_none());
            assert_eq!(loaded.retry_at, status.retry_at);
            assert_eq!(loaded.last_result.as_deref(), Some("error"));
        }
        assert!(admit_check(&path, false, status.retry_at.unwrap())
            .unwrap()
            .1
            .is_some());
    }

    #[test]
    fn check_lock_is_shared_with_a_separate_gui_process() {
        const FIXTURE_ENV: &str = "TODOLIST_TEST_UPDATE_CHECK_LOCK";
        if let Some(path) = std::env::var_os(FIXTURE_ENV) {
            let (status, lock) =
                admit_check(Path::new(&path), false, AUTOMATIC_INTERVAL_MS * 2).unwrap();
            assert!(lock.is_none());
            assert_eq!(status.last_attempt_at, Some(1_000));
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("checks.json");
        let (_, lock) = admit_check(&path, true, 1_000).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "stable_update::tests::check_lock_is_shared_with_a_separate_gui_process",
            ])
            .env(FIXTURE_ENV, &path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        drop(lock);
    }

    #[test]
    fn successful_last_api_request_persists_reset_without_scanning_partial_results() {
        let (result, paths, cooldown) = fetch_with_cooldown(vec![(
            200,
            "x-ratelimit-remaining: 0\r\nx-ratelimit-reset: 500\r\n",
            json!(vec![github_release("v0.2.24", false, false); PAGE_SIZE]),
        )]);
        assert!(result.unwrap_err().contains("限流"));
        assert_eq!(cooldown, Some(501_000));
        assert_eq!(paths.len(), 1);
        let (result, _, cooldown) = fetch_with_cooldown(vec![(
            200,
            "x-ratelimit-remaining: 0\r\nx-ratelimit-reset: 500\r\n",
            json!([github_release("v0.2.24", false, false)]),
        )]);
        assert_eq!(result.unwrap().version, Version::new(0, 2, 24));
        assert_eq!(cooldown, Some(501_000));
    }

    #[test]
    fn invalid_persisted_state_fails_closed_without_replacing_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("checks.json");
        fs::write(&path, b"broken state").unwrap();
        assert!(admit_check(&path, false, 1_000).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken state");
    }

    fn headers(values: &[(&str, &str)]) -> HeaderMap {
        values
            .iter()
            .map(|(name, value)| {
                (
                    reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                    value.parse().unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn server_retry_headers_are_authoritative_and_ordinary_403_has_no_server_cooldown() {
        let now = 1_000;
        assert_eq!(
            rate_limit_retry_at(
                StatusCode::FORBIDDEN,
                &HeaderMap::new(),
                "Resource not accessible",
                now
            ),
            None
        );
        assert_eq!(
            rate_limit_retry_at(
                StatusCode::FORBIDDEN,
                &headers(&[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "500")]),
                "",
                now
            ),
            Some(501_000)
        );
        assert_eq!(
            rate_limit_retry_at(
                StatusCode::TOO_MANY_REQUESTS,
                &headers(&[("retry-after", "120")]),
                "",
                now
            ),
            Some(121_000)
        );
        assert_eq!(
            rate_limit_retry_at(
                StatusCode::FORBIDDEN,
                &HeaderMap::new(),
                "You have exceeded a secondary rate limit",
                now
            ),
            Some(61_000)
        );
        assert_eq!(
            rate_limit_retry_at(
                StatusCode::TOO_MANY_REQUESTS,
                &headers(&[
                    ("retry-after", "180"),
                    ("x-ratelimit-remaining", "0"),
                    ("x-ratelimit-reset", "300")
                ]),
                "",
                now
            ),
            Some(301_000)
        );
        assert_eq!(
            rate_limit_retry_at(
                StatusCode::OK,
                &headers(&[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "500")]),
                "",
                now
            ),
            Some(501_000)
        );
        assert_eq!(
            rate_limit_retry_at(
                StatusCode::FORBIDDEN,
                &headers(&[
                    ("x-ratelimit-remaining", "12"),
                    ("x-ratelimit-reset", "500")
                ]),
                "Access denied",
                now
            ),
            None
        );
        assert_eq!(
            rate_limit_retry_at(
                StatusCode::TOO_MANY_REQUESTS,
                &headers(&[("retry-after", "invalid"), ("x-ratelimit-reset", "invalid")]),
                "",
                now
            ),
            Some(61_000)
        );
    }

    #[test]
    fn retry_after_http_date_and_expired_headers_are_handled() {
        let date = "Thu, 01 Jan 1970 00:05:00 GMT";
        assert_eq!(http_date_ms(date), Some(300_000));
        assert_eq!(http_date_ms("Fri, 29 Feb 2025 00:00:00 GMT"), None);
        assert_eq!(
            rate_limit_retry_at(
                StatusCode::TOO_MANY_REQUESTS,
                &headers(&[("retry-after", date)]),
                "",
                1_000
            ),
            Some(300_000)
        );
        assert_eq!(
            rate_limit_retry_at(
                StatusCode::TOO_MANY_REQUESTS,
                &headers(&[
                    ("retry-after", "0"),
                    ("x-ratelimit-remaining", "0"),
                    ("x-ratelimit-reset", "0")
                ]),
                "",
                1_000
            ),
            Some(61_000)
        );
    }

    #[test]
    fn failed_check_preserves_last_success_after_reload() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("checks.json");
        let mut status = UpdateCheckStatus {
            last_attempt_at: Some(1_000),
            last_result: Some("latest".into()),
            ..UpdateCheckStatus::default()
        };
        finish_check_status(&mut status, None, 1_500);
        assert_eq!(status.last_checked_at, Some(1_500));
        write_check_status(&path, &status).unwrap();

        let mut status = read_check_status(&path).unwrap();
        status.last_attempt_at = Some(61_000);
        status.last_result = Some("error".into());
        finish_check_status(&mut status, None, 62_000);
        write_check_status(&path, &status).unwrap();
        let status = read_check_status(&path).unwrap();
        assert_eq!(status.last_attempt_at, Some(61_000));
        assert_eq!(status.last_checked_at, Some(1_500));
        assert_eq!(status.last_result.as_deref(), Some("error"));

        let mut never_succeeded = UpdateCheckStatus {
            last_result: Some("error".into()),
            ..UpdateCheckStatus::default()
        };
        finish_check_status(&mut never_succeeded, None, 1_000);
        assert!(never_succeeded.last_checked_at.is_none());
        never_succeeded.last_result = Some("available".into());
        finish_check_status(&mut never_succeeded, None, 2_000);
        assert_eq!(never_succeeded.last_checked_at, Some(2_000));
    }

    #[test]
    fn repeated_true_limit_failures_back_off_after_reload_but_403_does_not() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("checks.json");
        let mut status = UpdateCheckStatus {
            last_result: Some("error".into()),
            ..UpdateCheckStatus::default()
        };
        finish_check_status(&mut status, Some(61_000), 1_000);
        assert_eq!(status.retry_at, Some(61_000));
        write_check_status(&path, &status).unwrap();
        let mut status = read_check_status(&path).unwrap();
        finish_check_status(&mut status, Some(121_000), 61_000);
        assert_eq!(status.retry_at, Some(181_000));
        finish_check_status(&mut status, Some(999_000), 181_000);
        assert_eq!(status.retry_at, Some(999_000));
        finish_check_status(&mut status, None, 999_000);
        assert_eq!(status.retry_at, None);
        assert_eq!(status.rate_limit_failures, 0);
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
            Instant::now(),
            || 1_000,
            &mut None,
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
