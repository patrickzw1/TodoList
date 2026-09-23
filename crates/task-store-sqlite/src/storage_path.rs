use std::path::{Path, PathBuf};

pub const IS_PRODUCTION: bool = cfg!(feature = "production");
pub const APP_IDENTIFIER: &str = if IS_PRODUCTION {
    "app.todolist.desktop"
} else {
    "app.todolist.desktop.dev"
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageClient {
    Desktop,
    Mcp,
}

fn default_database_path(data_directory: &Path, production: bool) -> PathBuf {
    data_directory
        .join(if production {
            "app.todolist.desktop"
        } else {
            "app.todolist.desktop.dev"
        })
        .join("todolist.sqlite")
}

#[cfg(windows)]
fn shared_database_path(home_directory: &Path, production: bool) -> PathBuf {
    home_directory
        .join(".todolist")
        .join(if production {
            "app.todolist.desktop"
        } else {
            "app.todolist.desktop.dev"
        })
        .join("todolist.sqlite")
}

#[cfg(windows)]
fn legacy_path_is_unredirected(legacy: &Path) -> Result<bool, String> {
    let parent = legacy
        .parent()
        .ok_or_else(|| "TodoList legacy database path has no parent directory".to_string())?;
    // A read of a packaged app's AppData file can fall through to the original
    // file, while a later write is redirected. Probe a fresh, disposable file.
    let probe = tempfile::Builder::new()
        .prefix(".todolist-storage-probe-")
        .tempfile_in(parent)
        .map_err(|error| error.to_string())?;
    let physical = probe
        .path()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let expected = probe.path().to_string_lossy().into_owned();
    let resolved = physical.to_string_lossy();
    let resolved = resolved.strip_prefix(r"\\?\").unwrap_or(&resolved);
    probe.close().map_err(|error| error.to_string())?;
    Ok(resolved.eq_ignore_ascii_case(&expected))
}

#[cfg(windows)]
fn windows_database_path(
    home_directory: &Path,
    data_directory: &Path,
    production: bool,
    client: StorageClient,
) -> Result<PathBuf, String> {
    let shared = shared_database_path(home_directory, production);
    if shared.is_file() {
        return Ok(shared);
    }
    if shared.exists() {
        return Err("TodoList shared database path is not a regular file".into());
    }
    let legacy = default_database_path(data_directory, production);
    if legacy.is_file() {
        if legacy_path_is_unredirected(&legacy)? {
            return Ok(legacy);
        }
        return Err(
            "TodoList legacy database is redirected to a different physical path; migrate both data stores before using this build"
                .into(),
        );
    }
    if legacy.exists()
        || legacy
            .parent()
            .is_some_and(|parent| parent.join("managed-files").exists())
        || shared.with_extension("sqlite-wal").exists()
        || shared.with_extension("sqlite-shm").exists()
        || shared
            .parent()
            .is_some_and(|parent| parent.join("managed-files").exists())
    {
        return Err(
            "Existing TodoList data needs an explicit migration to shared storage; no database was created"
                .into(),
        );
    }
    if client == StorageClient::Mcp {
        return Err(
            "Open TodoList desktop once to initialize shared storage before using MCP".into(),
        );
    }
    Ok(shared)
}

/// Desktop and MCP use the same build channel, including optimized local builds.
pub fn database_path(client: StorageClient) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("TODOLIST_DB_PATH") {
        return Ok(PathBuf::from(path));
    }
    let data_directory = dirs::data_dir()
        .ok_or_else(|| "Could not locate the application data directory".to_string())?;
    #[cfg(windows)]
    {
        let home_directory = dirs::home_dir()
            .ok_or_else(|| "Could not locate the user profile directory".to_string())?;
        windows_database_path(&home_directory, &data_directory, IS_PRODUCTION, client)
    }
    #[cfg(not(windows))]
    {
        let _ = client;
        Ok(default_database_path(&data_directory, IS_PRODUCTION))
    }
}

pub fn managed_files_path(database: &Path) -> Result<PathBuf, String> {
    let parent = database
        .parent()
        .ok_or_else(|| "TodoList database path has no parent directory".to_string())?;
    Ok(parent.join("managed-files"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SqliteTaskStore;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    use task_core::{Project, Workspace};

    #[test]
    fn development_writes_leave_the_production_database_unchanged() {
        let root = std::env::temp_dir().join(format!(
            "todolist-isolation-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let production = default_database_path(&root, true);
        let development = default_database_path(&root, false);
        assert_ne!(production, development);
        assert_eq!(
            production,
            root.join("app.todolist.desktop/todolist.sqlite")
        );
        assert_eq!(
            development,
            root.join("app.todolist.desktop.dev/todolist.sqlite")
        );
        for path in [&production, &development] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        let production_store = SqliteTaskStore::open(&production).unwrap();
        let workspace = Workspace {
            version: 1,
            projects: vec![Project {
                id: "daily".into(),
                name: "Daily work".into(),
                color: "#1264f4".into(),
            }],
            tasks: vec![],
        };
        production_store.save_workspace(&workspace).unwrap();
        let original = fs::read(&production).unwrap();
        let development_store = SqliteTaskStore::open(&development).unwrap();
        assert!(development_store.load_workspace().unwrap().is_none());
        let mut development_workspace = workspace.clone();
        development_workspace.projects[0].name = "Development only".into();
        development_store
            .save_workspace(&development_workspace)
            .unwrap();
        assert_eq!(fs::read(&production).unwrap(), original);
        assert_eq!(
            production_store.load_workspace().unwrap().unwrap().projects[0].name,
            "Daily work"
        );
        assert_eq!(
            development_store
                .load_workspace()
                .unwrap()
                .unwrap()
                .projects[0]
                .name,
            "Development only"
        );
        drop((development_store, production_store));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn build_channel_matches_the_application_identifier() {
        assert_eq!(
            default_database_path(Path::new("data"), IS_PRODUCTION),
            Path::new("data")
                .join(APP_IDENTIFIER)
                .join("todolist.sqlite")
        );
    }

    #[cfg(windows)]
    #[test]
    fn new_windows_store_waits_for_desktop_and_keeps_channels_separate() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("profile");
        let roaming = root.path().join("roaming");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&roaming).unwrap();

        assert!(windows_database_path(&home, &roaming, true, StorageClient::Mcp).is_err());
        let production =
            windows_database_path(&home, &roaming, true, StorageClient::Desktop).unwrap();
        let development =
            windows_database_path(&home, &roaming, false, StorageClient::Desktop).unwrap();
        assert_eq!(
            production,
            home.join(".todolist/app.todolist.desktop/todolist.sqlite")
        );
        assert_eq!(
            development,
            home.join(".todolist/app.todolist.desktop.dev/todolist.sqlite")
        );
        assert_ne!(production, development);
        fs::create_dir_all(production.parent().unwrap()).unwrap();
        let store = SqliteTaskStore::open(&production).unwrap();
        assert_eq!(
            windows_database_path(&home, &roaming, true, StorageClient::Mcp).unwrap(),
            production
        );
        assert_eq!(
            managed_files_path(&production).unwrap(),
            production.parent().unwrap().join("managed-files")
        );
        assert!(store.load_workspace().unwrap().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn existing_windows_store_stays_on_legacy_until_shared_database_is_present() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("profile");
        let roaming = root.path().join("roaming");
        let legacy = default_database_path(&roaming, true);
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, b"isolated legacy fixture").unwrap();

        for client in [StorageClient::Desktop, StorageClient::Mcp] {
            assert_eq!(
                windows_database_path(&home, &roaming, true, client).unwrap(),
                legacy
            );
        }
        let shared = shared_database_path(&home, true);
        fs::create_dir_all(shared.parent().unwrap()).unwrap();
        fs::write(&shared, b"isolated shared fixture").unwrap();
        for client in [StorageClient::Desktop, StorageClient::Mcp] {
            assert_eq!(
                windows_database_path(&home, &roaming, true, client).unwrap(),
                shared
            );
        }
        assert!(legacy.exists());
    }

    #[cfg(windows)]
    #[test]
    fn orphaned_managed_files_do_not_start_an_empty_database() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("profile");
        let roaming = root.path().join("roaming");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(
            default_database_path(&roaming, true)
                .parent()
                .unwrap()
                .join("managed-files"),
        )
        .unwrap();
        assert!(
            windows_database_path(&home, &roaming, true, StorageClient::Desktop)
                .unwrap_err()
                .contains("explicit migration")
        );
    }
}
