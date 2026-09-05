use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use task_core::Workspace;
use tempfile::NamedTempFile;

const BACKUP_SCHEMA_VERSION: u32 = 1;
const MAX_BACKUP_BYTES: u64 = 25 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceBackup {
    schema_version: u32,
    exported_at: u64,
    app_version: String,
    workspace: Workspace,
}

fn validate_workspace(workspace: &Workspace) -> Result<(), String> {
    workspace.validate()?;
    let mut project_ids = HashSet::new();
    for project in &workspace.projects {
        if project.id.trim().is_empty() || project.name.trim().is_empty() {
            return Err("Backup contains a project with an empty id or name".to_string());
        }
        if !project_ids.insert(&project.id) {
            return Err(format!(
                "Backup contains duplicate project id '{}'",
                project.id
            ));
        }
    }

    let mut task_ids = HashSet::new();
    for task in &workspace.tasks {
        if task.id.trim().is_empty() || task.title.trim().is_empty() {
            return Err("Backup contains a task with an empty id or title".to_string());
        }
        if !task_ids.insert(&task.id) {
            return Err(format!("Backup contains duplicate task id '{}'", task.id));
        }
    }
    Ok(())
}

fn selected_file(path: String) -> Result<PathBuf, String> {
    if path.trim().is_empty() {
        return Err("No backup file was selected".to_string());
    }
    Ok(PathBuf::from(path))
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Backup path has no parent directory".to_string())?;
    let parent_metadata = fs::metadata(parent).map_err(|error| error.to_string())?;
    if !parent_metadata.is_dir() {
        return Err("Backup parent is not a directory".to_string());
    }
    if path.exists() {
        let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("Backup target is not a regular file and will not be replaced".to_string());
        }
    }

    let mut temporary = NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    temporary
        .write_all(content)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| error.to_string())?;
    temporary
        .persist(path)
        .map_err(|error| error.error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn export_workspace_backup(path: String, mut workspace: Workspace) -> Result<(), String> {
    workspace.trim_activity();
    validate_workspace(&workspace)?;
    let backup = WorkspaceBackup {
        schema_version: BACKUP_SCHEMA_VERSION,
        exported_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        workspace,
    };
    let content = serde_json::to_vec_pretty(&backup).map_err(|error| error.to_string())?;
    if content.len() as u64 > MAX_BACKUP_BYTES {
        return Err("Backup is larger than the 25 MB safety limit".to_string());
    }
    atomic_write(&selected_file(path)?, &content)
}

#[tauri::command]
pub fn read_workspace_backup(path: String) -> Result<WorkspaceBackup, String> {
    let path = selected_file(path)?;
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Selected backup is not a regular file".to_string());
    }
    if metadata.len() > MAX_BACKUP_BYTES {
        return Err("Backup is larger than the 25 MB safety limit".to_string());
    }
    let mut backup: WorkspaceBackup =
        serde_json::from_slice(&fs::read(&path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Backup is not valid TodoList JSON: {error}"))?;
    if backup.schema_version != BACKUP_SCHEMA_VERSION {
        return Err(format!(
            "Unsupported backup schema version {}; expected {}",
            backup.schema_version, BACKUP_SCHEMA_VERSION
        ));
    }
    backup.workspace.trim_activity();
    validate_workspace(&backup.workspace)?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use task_core::{Project, Workspace};

    fn workspace() -> Workspace {
        Workspace {
            version: 4,
            projects: vec![Project {
                id: "project-1".into(),
                name: "TodoList".into(),
                color: "#1264f4".into(),
            }],
            tasks: vec![],
        }
    }

    #[test]
    fn round_trips_only_the_selected_backup_file() {
        let directory = tempfile::tempdir().unwrap();
        let backup_path = directory.path().join("backup.json");
        let unrelated_path = directory.path().join("keep.txt");
        fs::write(&backup_path, "old backup").unwrap();
        fs::write(&unrelated_path, "keep me").unwrap();

        export_workspace_backup(backup_path.to_string_lossy().into_owned(), workspace()).unwrap();
        let backup = read_workspace_backup(backup_path.to_string_lossy().into_owned()).unwrap();

        assert_eq!(backup.schema_version, BACKUP_SCHEMA_VERSION);
        assert_eq!(backup.workspace.version, 4);
        assert_eq!(fs::read_to_string(unrelated_path).unwrap(), "keep me");
    }

    #[test]
    fn rejects_future_schemas_and_invalid_project_references() {
        let directory = tempfile::tempdir().unwrap();
        let backup_path = directory.path().join("future.json");
        let mut value = serde_json::to_value(WorkspaceBackup {
            schema_version: BACKUP_SCHEMA_VERSION + 1,
            exported_at: 0,
            app_version: "future".into(),
            workspace: workspace(),
        })
        .unwrap();
        fs::write(&backup_path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            read_workspace_backup(backup_path.to_string_lossy().into_owned())
                .unwrap_err()
                .contains("Unsupported backup schema")
        );

        value["schemaVersion"] = serde_json::json!(BACKUP_SCHEMA_VERSION);
        value["workspace"]["tasks"] = serde_json::json!([{
            "id": "task-1", "projectId": "missing", "title": "Task", "description": "",
            "status": "todo", "priority": "medium", "dueLabel": "未安排", "dueDate": "9999-12-31",
            "tags": [], "source": "手动创建", "archived": false, "pinned": false, "version": 1,
            "subtasks": [], "acceptanceCriteria": [], "dependencies": [], "activity": []
        }]);
        fs::write(&backup_path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            read_workspace_backup(backup_path.to_string_lossy().into_owned())
                .unwrap_err()
                .contains("unknown project")
        );
    }
}
