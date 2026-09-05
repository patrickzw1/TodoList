# Data backup and restore

TodoList exports one UTF-8 JSON file with `schemaVersion`, export time, app version and the current workspace. Schema version 1 contains projects, tasks, subtasks, acceptance criteria, dependencies, pin/archive state and the newest 100 activity entries per task.

Desktop builds use the operating system's native open/save dialogs. Export writes only the exact file selected by the user, rejects symlink or non-file targets, writes through a temporary file in the same directory and leaves neighboring files untouched. Import only reads the selected regular file and never modifies the source backup.

Both the TypeScript preview layer and the Rust desktop layer reject invalid JSON, files larger than 25 MB, unsupported schema versions, duplicate project/task ids and tasks that reference missing projects. Selecting a backup does not change data: the UI first shows its timestamp and project/task counts, and replacement requires a separate explicit confirmation.

When confirmed, the imported workspace is reapplied to the latest persisted workspace version. The version stored inside the backup is never used to overwrite the current database version, so the existing optimistic conflict handling remains active.
