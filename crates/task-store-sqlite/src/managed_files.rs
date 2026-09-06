use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use task_core::{ManagedFile, Workspace};
use tempfile::NamedTempFile;
use uuid::Uuid;

pub const MAX_MANAGED_FILE_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedFilePayload {
    pub storage_key: String,
    pub data: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoredStorageKey {
    pub original_storage_key: String,
    pub new_storage_key: String,
}

pub fn media_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "zip" => "application/zip",
        "txt" | "md" => "text/plain",
        _ => "application/octet-stream",
    }
}

fn safe_extension(path: &Path) -> String {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return String::new();
    };
    if extension.len() > 16
        || !extension
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return String::new();
    }
    format!(".{}", extension.to_ascii_lowercase())
}

pub fn resolve_storage_key(root: &Path, storage_key: &str) -> Result<PathBuf, String> {
    let relative = Path::new(storage_key);
    if relative.is_absolute()
        || relative.components().count() != 2
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Managed file key is invalid".to_string());
    }
    let mut components = relative.components();
    let folder = components
        .next()
        .and_then(|part| match part {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .unwrap_or("");
    if !matches!(folder, "attachments" | "images") {
        return Err("Managed file key has an unsupported collection".to_string());
    }
    let name = relative
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Invalid file name")?;
    if name.contains(['<', '>', ':', '"', '|', '?', '*']) || name.ends_with(['.', ' ']) {
        return Err("Managed file key contains an invalid file name".into());
    }
    for directory in [root.to_path_buf(), root.join(folder)] {
        if let Ok(metadata) = fs::symlink_metadata(&directory) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err("Managed file directories must not be links".into());
            }
        }
    }
    Ok(root.join(relative))
}

pub fn import_into_root(root: &Path, source: &Path, kind: &str) -> Result<ManagedFile, String> {
    let collection = match kind {
        "attachment" => "attachments",
        "image" => "images",
        _ => return Err("Managed file kind must be attachment or image".to_string()),
    };
    if !source.is_absolute() {
        return Err("Source path must be absolute".into());
    }
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("Could not read the selected file: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("The selected item is not a regular file".to_string());
    }
    if metadata.len() > MAX_MANAGED_FILE_BYTES {
        return Err("The selected file is larger than the 100 MB limit".to_string());
    }
    let original_name = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "The selected file name is not valid Unicode".to_string())?
        .to_string();
    let detected_type = media_type(source).to_string();
    if kind == "image" && !detected_type.starts_with("image/") {
        return Err("The selected file is not a supported image".to_string());
    }

    let id = Uuid::new_v4().to_string();
    let storage_key = format!("{collection}/{id}{}", safe_extension(source));
    let destination = resolve_storage_key(root, &storage_key)?;
    let parent = destination
        .parent()
        .ok_or_else(|| "Managed file destination has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    let input = File::open(source).map_err(|error| error.to_string())?;
    let size = std::io::copy(&mut input.take(MAX_MANAGED_FILE_BYTES + 1), &mut temporary)
        .map_err(|error| error.to_string())?;
    if size > MAX_MANAGED_FILE_BYTES {
        return Err("The selected file is larger than the 100 MB limit".into());
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temporary
        .persist(&destination)
        .map_err(|error| error.error.to_string())?;

    Ok(ManagedFile {
        id,
        original_name,
        media_type: detected_type,
        size,
        storage_key,
        added_at: chrono::Utc::now().to_rfc3339(),
    })
}

fn referenced_keys(workspace: &Workspace) -> HashSet<&str> {
    workspace
        .tasks
        .iter()
        .flat_map(|task| task.attachments.iter().chain(&task.images))
        .map(|file| file.storage_key.as_str())
        .collect()
}

pub fn payloads_for_workspace(
    root: &Path,
    workspace: &Workspace,
) -> Result<Vec<ManagedFilePayload>, String> {
    let mut payloads = Vec::new();
    for storage_key in referenced_keys(workspace) {
        let path = resolve_storage_key(root, storage_key)?;
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| format!("Managed file '{storage_key}' is missing"))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_MANAGED_FILE_BYTES
        {
            return Err(format!("Managed file '{storage_key}' is unavailable"));
        }
        payloads.push(ManagedFilePayload {
            storage_key: storage_key.to_string(),
            data: STANDARD.encode(fs::read(path).map_err(|error| error.to_string())?),
        });
    }
    Ok(payloads)
}

pub fn restore_payloads(
    root: &Path,
    payloads: &[ManagedFilePayload],
) -> Result<Vec<RestoredStorageKey>, String> {
    let unique: HashSet<_> = payloads.iter().map(|file| &file.storage_key).collect();
    if unique.len() != payloads.len() {
        return Err("Backup contains duplicate managed file keys".into());
    }
    let mut restored = Vec::new();
    let mut created = Vec::new();
    let result = (|| {
        for payload in payloads {
            let old_path = resolve_storage_key(root, &payload.storage_key)?;
            let collection = Path::new(&payload.storage_key)
                .components()
                .next()
                .and_then(|part| match part {
                    Component::Normal(value) => value.to_str(),
                    _ => None,
                })
                .unwrap();
            let extension = safe_extension(&old_path);
            let new_storage_key = format!("{collection}/{}{}", Uuid::new_v4(), extension);
            let destination = resolve_storage_key(root, &new_storage_key)?;
            let parent = destination.parent().unwrap();
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            let bytes = STANDARD
                .decode(&payload.data)
                .map_err(|error| format!("Backup file data is invalid: {error}"))?;
            if bytes.len() as u64 > MAX_MANAGED_FILE_BYTES {
                return Err("A restored managed file exceeds the 100 MB limit".to_string());
            }
            let mut temporary = NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
            temporary
                .write_all(&bytes)
                .and_then(|_| temporary.as_file().sync_all())
                .map_err(|error| error.to_string())?;
            temporary
                .persist(&destination)
                .map_err(|error| error.error.to_string())?;
            created.push(destination);
            restored.push(RestoredStorageKey {
                original_storage_key: payload.storage_key.clone(),
                new_storage_key,
            });
        }
        Ok(restored)
    })();
    if result.is_err() {
        for path in created {
            let _ = fs::remove_file(path);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use task_core::{Priority, Project, Task, TaskStatus};

    fn workspace_with(file: ManagedFile) -> Workspace {
        Workspace {
            version: 1,
            projects: vec![Project {
                id: "p".into(),
                name: "P".into(),
                color: "#000".into(),
            }],
            tasks: vec![Task {
                id: "t".into(),
                project_id: "p".into(),
                title: "T".into(),
                description: String::new(),
                status: TaskStatus::Todo,
                priority: Priority::Medium,
                due_label: "未安排".into(),
                due_date: "9999-12-31".into(),
                tags: vec![],
                source: "user".into(),
                archived: false,
                pinned: false,
                version: 1,
                subtasks: vec![],
                acceptance_criteria: vec![],
                attachments: vec![file],
                images: vec![],
                dependencies: vec![],
                activity: vec![],
            }],
        }
    }

    #[test]
    fn atomic_import_and_committed_cleanup_preserve_staged_files_and_originals() {
        let source_dir = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let store = crate::SqliteTaskStore::open(root.path().join("tasks.sqlite")).unwrap();
        let files_root = store.managed_files_root();
        let original = source_dir.path().join("report.final.pdf");
        fs::write(&original, b"content").unwrap();
        let staged = import_into_root(&files_root, &original, "attachment").unwrap();
        let mut initial = workspace_with(staged.clone());
        initial.tasks[0].attachments.clear();
        store.save_workspace(&initial).unwrap();
        let file = store
            .import_task_file("t", &original, "attachment")
            .unwrap();
        let managed = resolve_storage_key(&files_root, &file.storage_key).unwrap();
        assert_eq!(fs::read(&managed).unwrap(), b"content");
        assert_eq!(fs::read(&original).unwrap(), b"content");
        let current = store.load_workspace().unwrap().unwrap();
        assert_eq!(current.tasks[0].attachments[0].id, file.id);
        assert_eq!(
            payloads_for_workspace(&files_root, &current).unwrap()[0].data,
            STANDARD.encode(b"content")
        );
        store.cleanup_managed_files().unwrap();
        assert!(managed.exists());
        let mut removed = current.clone();
        removed.tasks.clear();
        // A rejected stale save must not queue deletion.
        assert!(store.save_workspace(&removed).is_err());
        store.cleanup_managed_files().unwrap();
        assert!(managed.exists());
        removed.version += 1;
        store.save_workspace(&removed).unwrap();
        // Cleanup survives a restart after the workspace commit.
        let reopened = crate::SqliteTaskStore::open(root.path().join("tasks.sqlite")).unwrap();
        reopened.cleanup_managed_files().unwrap();
        assert!(!managed.exists());
        assert!(resolve_storage_key(&files_root, &staged.storage_key)
            .unwrap()
            .exists());
        assert!(original.exists());
        assert!(store
            .import_task_file("deleted-task", &original, "attachment")
            .is_err());
    }

    #[test]
    fn cleanup_checks_latest_references_before_deleting() {
        let root = tempfile::tempdir().unwrap();
        let store = crate::SqliteTaskStore::open(root.path().join("tasks.sqlite")).unwrap();
        let original = root.path().join("image.png");
        fs::write(&original, b"image").unwrap();
        let file = import_into_root(&store.managed_files_root(), &original, "image").unwrap();
        let mut workspace = workspace_with(file.clone());
        store.save_workspace(&workspace).unwrap();
        store
            .mutate_workspace(|mut current| {
                current.tasks[0].attachments.clear();
                current.version += 1;
                Ok(current)
            })
            .unwrap();
        workspace.version = 3;
        store.save_workspace(&workspace).unwrap();
        store.cleanup_managed_files().unwrap();
        assert!(
            resolve_storage_key(&store.managed_files_root(), &file.storage_key)
                .unwrap()
                .exists()
        );
    }

    #[test]
    fn rejects_unsafe_keys_and_invalid_imports() {
        let root = tempfile::tempdir().unwrap();
        for key in [
            "../outside.txt",
            "attachments/../outside.txt",
            "attachments/file.txt:stream",
            "images/file.",
            "other/file.txt",
        ] {
            assert!(resolve_storage_key(root.path(), key).is_err(), "{key}");
        }
        let source = root.path().join("report.txt");
        fs::write(&source, b"text").unwrap();
        assert!(import_into_root(root.path(), &source, "image").is_err());
        assert!(import_into_root(root.path(), root.path(), "attachment").is_err());
        assert!(import_into_root(root.path(), Path::new("relative.txt"), "attachment").is_err());
    }

    #[test]
    fn backup_restore_uses_new_storage_keys_without_overwriting_existing_files() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("attachments/original.pdf");
        fs::create_dir_all(old.parent().unwrap()).unwrap();
        fs::write(&old, b"current").unwrap();
        let restored = restore_payloads(
            root.path(),
            &[ManagedFilePayload {
                storage_key: "attachments/original.pdf".into(),
                data: STANDARD.encode(b"backup"),
            }],
        )
        .unwrap();
        assert_ne!(restored[0].new_storage_key, "attachments/original.pdf");
        assert_eq!(fs::read(old).unwrap(), b"current");
        assert_eq!(
            fs::read(resolve_storage_key(root.path(), &restored[0].new_storage_key).unwrap())
                .unwrap(),
            b"backup"
        );
    }
}
