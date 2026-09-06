use std::path::{Path, PathBuf};

pub const IS_PRODUCTION: bool = cfg!(feature = "production");
pub const APP_IDENTIFIER: &str = if IS_PRODUCTION {
    "app.todolist.desktop"
} else {
    "app.todolist.desktop.dev"
};

fn default_database_path(data_directory: &Path, production: bool) -> PathBuf {
    data_directory
        .join(if production {
            "app.todolist.desktop"
        } else {
            "app.todolist.desktop.dev"
        })
        .join("todolist.sqlite")
}

/// Desktop and MCP use the same build channel, including optimized local builds.
pub fn database_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("TODOLIST_DB_PATH") {
        return Ok(PathBuf::from(path));
    }
    let data_directory = dirs::data_dir()
        .ok_or_else(|| "Could not locate the application data directory".to_string())?;
    Ok(default_database_path(&data_directory, IS_PRODUCTION))
}

pub fn managed_files_path() -> Result<PathBuf, String> {
    let database = database_path()?;
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
}
