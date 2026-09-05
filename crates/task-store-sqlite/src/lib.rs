use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use task_core::Workspace;

const MAX_IDEMPOTENCY_RECORDS: i64 = 1_000;

#[derive(Debug, Clone)]
pub struct SqliteTaskStore {
    database_path: PathBuf,
}

#[derive(Debug)]
pub struct IdempotentMutationResult {
    pub workspace: Workspace,
    pub entity_id: String,
    pub replayed: bool,
}

impl SqliteTaskStore {
    pub fn open(database_path: impl AsRef<Path>) -> Result<Self, String> {
        let store = Self {
            database_path: database_path.as_ref().to_path_buf(),
        };
        let connection = store.connection()?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 3000;
             CREATE TABLE IF NOT EXISTS workspace_snapshot (
               id INTEGER PRIMARY KEY CHECK (id = 1),
               version INTEGER NOT NULL,
               payload_json TEXT NOT NULL,
               updated_at INTEGER NOT NULL
             );",
            )
            .map_err(|error| error.to_string())?;
        Self::migrate(&connection)?;
        Ok(store)
    }

    fn migrate(connection: &Connection) -> Result<(), String> {
        let mut schema_version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|error| error.to_string())?;
        if schema_version < 1 {
            // v1 removes an unused table that duplicated the full workspace on every write.
            // VACUUM runs before the version bump so an interrupted compaction is retried.
            connection
                .execute_batch("DROP TABLE IF EXISTS task_event; VACUUM; PRAGMA user_version = 1;")
                .map_err(|error| error.to_string())?;
            schema_version = 1;
        }

        if schema_version < 2 {
            connection
                .execute_batch(
                    "CREATE TABLE IF NOT EXISTS mcp_idempotency (
                       operation TEXT NOT NULL,
                       request_id TEXT NOT NULL,
                       request_fingerprint TEXT NOT NULL,
                       entity_id TEXT NOT NULL,
                       created_at INTEGER NOT NULL,
                       PRIMARY KEY (operation, request_id)
                     );
                     PRAGMA user_version = 2;",
                )
                .map_err(|error| error.to_string())?;
        }
        if schema_version < 3 {
            // One bounded clock survives task deletion and backup restoration.
            // Seed it from existing task versions, including old imported data.
            let highest = Self::load_from_connection(connection)?
                .map(|workspace| {
                    workspace
                        .tasks
                        .iter()
                        .map(|task| task.version)
                        .max()
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            connection
                .execute_batch(
                    "CREATE TABLE IF NOT EXISTS task_version_clock (
                   id INTEGER PRIMARY KEY CHECK (id = 1), high_water INTEGER NOT NULL
                 );",
                )
                .map_err(|error| error.to_string())?;
            connection
                .execute(
                    "INSERT INTO task_version_clock (id, high_water) VALUES (1, ?1)
                 ON CONFLICT(id) DO UPDATE SET high_water = MAX(high_water, excluded.high_water)",
                    params![highest],
                )
                .map_err(|error| error.to_string())?;
            connection
                .pragma_update(None, "user_version", 3)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn connection(&self) -> Result<Connection, String> {
        let connection =
            Connection::open(&self.database_path).map_err(|error| error.to_string())?;
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .map_err(|error| error.to_string())?;
        Ok(connection)
    }

    pub fn load_workspace(&self) -> Result<Option<Workspace>, String> {
        let connection = self.connection()?;
        Self::load_from_connection(&connection)
    }

    pub fn load_workspace_version(&self) -> Result<Option<u64>, String> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT version FROM workspace_snapshot WHERE id = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map(|version| version.map(|value| value as u64))
            .map_err(|error| error.to_string())
    }

    fn load_from_connection(connection: &Connection) -> Result<Option<Workspace>, String> {
        let payload: Option<String> = connection
            .query_row(
                "SELECT payload_json FROM workspace_snapshot WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        payload
            .map(|value| {
                let mut workspace: Workspace =
                    serde_json::from_str(&value).map_err(|error| error.to_string())?;
                workspace.trim_activity();
                Ok(workspace)
            })
            .transpose()
    }

    pub fn save_workspace(&self, workspace: &Workspace) -> Result<(), String> {
        let mut bounded_workspace = workspace.clone();
        bounded_workspace.trim_activity();
        bounded_workspace.validate()?;
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let current = Self::load_from_connection(&transaction)?;
        if let Some(current) = &current {
            if bounded_workspace.version <= current.version {
                return Err(format!(
                    "workspace version conflict: current={}, attempted={}; reload before saving",
                    current.version, bounded_workspace.version
                ));
            }
            bounded_workspace.validate_completion_changes(&current)?;
        }
        Self::persist_transaction(&transaction, &bounded_workspace, current.as_ref())?;
        transaction.commit().map_err(|error| error.to_string())
    }

    pub fn restore_workspace(&self, imported: &Workspace) -> Result<Workspace, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let current = Self::load_from_connection(&transaction)?;
        if let Some(current) = &current {
            if imported.version != current.version + 1 {
                return Err(format!(
                    "workspace version conflict: current={}; reload before restoring",
                    current.version
                ));
            }
        }
        let mut high_water: u64 = transaction
            .query_row(
                "SELECT high_water FROM task_version_clock WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        for task in imported
            .tasks
            .iter()
            .chain(current.iter().flat_map(|workspace| &workspace.tasks))
        {
            high_water = high_water.max(task.version);
        }
        // Versions cross the JS bridge and must remain exact integers.
        let next_version = high_water
            .checked_add(1)
            .filter(|version| *version <= 9_007_199_254_740_991)
            .ok_or_else(|| "Task version limit reached; cannot restore backup".to_string())?;
        let mut restored = imported.clone();
        for task in &mut restored.tasks {
            task.version = next_version;
        }
        restored.trim_activity();
        restored.validate()?;
        // Restoring a legacy backup preserves its data; subsequent completion
        // transitions are still checked by every ordinary write path.
        Self::persist_transaction(&transaction, &restored, current.as_ref())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(restored)
    }

    pub fn mutate_workspace<F>(&self, mutate: F) -> Result<Workspace, String>
    where
        F: FnOnce(Workspace) -> Result<Workspace, String>,
    {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let current = Self::load_from_connection(&transaction)?.ok_or_else(|| {
            "TodoList workspace is not initialized; open the desktop app once before using MCP"
                .to_string()
        })?;
        let mut updated = mutate(current.clone())?;
        updated.trim_activity();
        updated.validate()?;
        updated.validate_completion_changes(&current)?;
        Self::persist_transaction(&transaction, &updated, Some(&current))?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(updated)
    }

    pub fn mutate_workspace_idempotent<F>(
        &self,
        operation: &str,
        request_id: &str,
        request_fingerprint: &str,
        entity_id: &str,
        mutate: F,
    ) -> Result<IdempotentMutationResult, String>
    where
        F: FnOnce(Workspace) -> Result<Workspace, String>,
    {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT request_fingerprint, entity_id
                 FROM mcp_idempotency
                 WHERE operation = ?1 AND request_id = ?2",
                params![operation, request_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;

        if let Some((existing_fingerprint, existing_entity_id)) = existing {
            if existing_fingerprint != request_fingerprint {
                return Err(format!(
                    "idempotency conflict: request_id '{request_id}' was already used for different {operation} input"
                ));
            }
            let workspace = Self::load_from_connection(&transaction)?.ok_or_else(|| {
                "TodoList workspace is not initialized; open the desktop app once before using MCP"
                    .to_string()
            })?;
            transaction.commit().map_err(|error| error.to_string())?;
            return Ok(IdempotentMutationResult {
                workspace,
                entity_id: existing_entity_id,
                replayed: true,
            });
        }

        let current = Self::load_from_connection(&transaction)?.ok_or_else(|| {
            "TodoList workspace is not initialized; open the desktop app once before using MCP"
                .to_string()
        })?;
        let mut updated = mutate(current.clone())?;
        updated.trim_activity();
        updated.validate()?;
        updated.validate_completion_changes(&current)?;
        Self::persist_transaction(&transaction, &updated, Some(&current))?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs() as i64;
        transaction
            .execute(
                "INSERT INTO mcp_idempotency (
                   operation, request_id, request_fingerprint, entity_id, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![operation, request_id, request_fingerprint, entity_id, now],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "DELETE FROM mcp_idempotency
                 WHERE rowid IN (
                   SELECT rowid FROM mcp_idempotency
                   ORDER BY rowid DESC
                   LIMIT -1 OFFSET ?1
                 )",
                params![MAX_IDEMPOTENCY_RECORDS],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(IdempotentMutationResult {
            workspace: updated,
            entity_id: entity_id.to_string(),
            replayed: false,
        })
    }

    fn persist_transaction(
        transaction: &Transaction<'_>,
        workspace: &Workspace,
        previous: Option<&Workspace>,
    ) -> Result<(), String> {
        // Include removed tasks too: an older still-running MCP may have updated
        // their version without knowing about this clock.
        let high_water = workspace
            .tasks
            .iter()
            .chain(previous.into_iter().flat_map(|workspace| &workspace.tasks))
            .map(|task| task.version)
            .max()
            .unwrap_or(0);
        transaction
            .execute(
                "UPDATE task_version_clock SET high_water = MAX(high_water, ?1) WHERE id = 1",
                params![high_water],
            )
            .map_err(|error| error.to_string())?;
        let payload = serde_json::to_string(workspace).map_err(|error| error.to_string())?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs() as i64;
        transaction
            .execute(
            "INSERT INTO workspace_snapshot (id, version, payload_json, updated_at)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET version = excluded.version, payload_json = excluded.payload_json, updated_at = excluded.updated_at",
            params![workspace.version as i64, payload, now],
        )
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use task_core::{ActivityItem, Priority, Project, Task, TaskStatus, MAX_TASK_ACTIVITY_ITEMS};

    #[test]
    fn restore_clock_migrates_legacy_versions_and_retains_versions_of_removed_tasks() {
        let database_path = std::env::temp_dir().join(format!(
            "todolist-clock-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteTaskStore::open(&database_path).unwrap();
        let legacy: Workspace = serde_json::from_value(serde_json::json!({
            "version": 2, "projects": [{"id":"p","name":"P","color":"#fff"}],
            "tasks": [{"id":"t","projectId":"p","title":"Legacy","description":"", "status":"todo", "priority":"medium",
                "dueLabel":"", "dueDate":"", "tags":[], "source":"user", "pinned":false, "version":1000,
                "subtasks":[], "acceptanceCriteria":[], "dependencies":[], "activity":[]}]
        })).unwrap();
        store.save_workspace(&legacy).unwrap();
        let connection = store.connection().unwrap();
        connection
            .execute_batch("DROP TABLE task_version_clock; PRAGMA user_version = 2;")
            .unwrap();
        drop(connection);
        let migrated = SqliteTaskStore::open(&database_path).unwrap();
        assert_eq!(
            migrated.load_workspace().unwrap().unwrap().tasks[0].version,
            1000
        );
        // Simulate an already-running old MCP that does not maintain the new clock.
        let mut externally_updated = legacy.clone();
        externally_updated.tasks[0].version = 1001;
        externally_updated.version += 1;
        migrated
            .connection()
            .unwrap()
            .execute(
                "UPDATE workspace_snapshot SET version = ?1, payload_json = ?2 WHERE id = 1",
                params![
                    externally_updated.version,
                    serde_json::to_string(&externally_updated).unwrap()
                ],
            )
            .unwrap();
        migrated
            .mutate_workspace(|mut current| {
                current.version += 1;
                current.tasks.clear();
                Ok(current)
            })
            .unwrap();
        let mut backup = legacy;
        backup.version = migrated.load_workspace_version().unwrap().unwrap() + 1;
        backup.tasks[0].version = 1;
        assert_eq!(
            migrated.restore_workspace(&backup).unwrap().tasks[0].version,
            1002
        );
        let rows: u64 = migrated
            .connection()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM task_version_clock", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 1);
        drop(migrated);
        drop(store);
        let _ = std::fs::remove_file(database_path.with_extension("sqlite-shm"));
        let _ = std::fs::remove_file(database_path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(database_path);
    }

    #[test]
    fn saves_and_loads_a_workspace_snapshot() {
        let database_path = std::env::temp_dir().join(format!(
            "todolist-store-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let store = SqliteTaskStore::open(&database_path).expect("open store");
        let workspace = Workspace {
            version: 7,
            projects: vec![Project {
                id: "project-1".into(),
                name: "TodoList".into(),
                color: "#1264f4".into(),
            }],
            tasks: vec![],
        };

        store.save_workspace(&workspace).expect("save workspace");
        let loaded = store
            .load_workspace()
            .expect("load workspace")
            .expect("workspace exists");

        assert_eq!(loaded.version, 7);
        assert_eq!(loaded.projects[0].name, "TodoList");
        assert_eq!(store.load_workspace_version().unwrap(), Some(7));

        let stale_error = store
            .save_workspace(&workspace)
            .expect_err("reject stale save");
        assert!(stale_error.contains("version conflict"));

        drop(store);
        let _ = std::fs::remove_file(database_path.with_extension("sqlite-shm"));
        let _ = std::fs::remove_file(database_path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(database_path);
    }

    #[test]
    fn mutates_workspace_atomically() {
        let database_path = std::env::temp_dir().join(format!(
            "todolist-mutate-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let store = SqliteTaskStore::open(&database_path).expect("open store");
        store
            .save_workspace(&Workspace {
                version: 1,
                projects: vec![Project {
                    id: "project-1".into(),
                    name: "TodoList".into(),
                    color: "#1264f4".into(),
                }],
                tasks: vec![],
            })
            .expect("seed workspace");

        let updated = store
            .mutate_workspace(|mut workspace| {
                workspace.version += 1;
                Ok(workspace)
            })
            .expect("mutate workspace");

        assert_eq!(updated.version, 2);
        assert_eq!(store.load_workspace().unwrap().unwrap().version, 2);

        drop(store);
        let _ = std::fs::remove_file(database_path.with_extension("sqlite-shm"));
        let _ = std::fs::remove_file(database_path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(database_path);
    }

    #[test]
    fn bounds_activity_before_persisting_a_workspace() {
        let database_path = std::env::temp_dir().join(format!(
            "todolist-bounded-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteTaskStore::open(&database_path).expect("open store");
        let activity = (0..MAX_TASK_ACTIVITY_ITEMS + 5)
            .map(|index| ActivityItem {
                id: index.to_string(),
                action: "edit".into(),
                actor: "user".into(),
                at: "now".into(),
            })
            .collect();
        store
            .save_workspace(&Workspace {
                version: 1,
                projects: vec![Project {
                    id: "project-1".into(),
                    name: "TodoList".into(),
                    color: "#1264f4".into(),
                }],
                tasks: vec![Task {
                    id: "task-1".into(),
                    project_id: "project-1".into(),
                    title: "Task".into(),
                    description: String::new(),
                    status: TaskStatus::Todo,
                    priority: Priority::Medium,
                    due_label: "Today".into(),
                    due_date: "2026-09-04".into(),
                    tags: vec![],
                    source: "user".into(),
                    archived: false,
                    pinned: false,
                    version: 1,
                    subtasks: vec![],
                    acceptance_criteria: vec![],
                    dependencies: vec![],
                    activity,
                }],
            })
            .expect("save workspace");

        let loaded = store.load_workspace().unwrap().unwrap();
        assert_eq!(loaded.tasks[0].activity.len(), MAX_TASK_ACTIVITY_ITEMS);
        assert_eq!(loaded.tasks[0].activity[0].id, "5");

        drop(store);
        let _ = std::fs::remove_file(database_path.with_extension("sqlite-shm"));
        let _ = std::fs::remove_file(database_path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(database_path);
    }

    #[test]
    fn persists_idempotent_replays_and_bounds_the_retry_ledger() {
        let database_path = std::env::temp_dir().join(format!(
            "todolist-idempotency-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteTaskStore::open(&database_path).expect("open store");
        store
            .save_workspace(&Workspace {
                version: 1,
                projects: vec![],
                tasks: vec![],
            })
            .expect("seed workspace");

        for index in 0..=MAX_IDEMPOTENCY_RECORDS {
            let request_id = format!("request-{index}");
            let entity_id = format!("entity-{index}");
            store
                .mutate_workspace_idempotent(
                    "create_test_item",
                    &request_id,
                    "same-fingerprint",
                    &entity_id,
                    |mut workspace| {
                        workspace.version += 1;
                        Ok(workspace)
                    },
                )
                .expect("record idempotent mutation");
        }

        let newest_request_id = format!("request-{MAX_IDEMPOTENCY_RECORDS}");
        let newest_entity_id = format!("entity-{MAX_IDEMPOTENCY_RECORDS}");
        let replay = store
            .mutate_workspace_idempotent(
                "create_test_item",
                &newest_request_id,
                "same-fingerprint",
                "unused-entity",
                |_| panic!("a replay must not run the mutation"),
            )
            .expect("replay latest request");
        assert!(replay.replayed);
        assert_eq!(replay.entity_id, newest_entity_id);

        let conflict = store
            .mutate_workspace_idempotent(
                "create_test_item",
                &newest_request_id,
                "different-fingerprint",
                "unused-entity",
                |workspace| Ok(workspace),
            )
            .expect_err("reject request id reuse with different input");
        assert!(conflict.contains("idempotency conflict"));

        let connection = Connection::open(&database_path).unwrap();
        let record_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM mcp_idempotency", [], |row| row.get(0))
            .unwrap();
        assert_eq!(record_count, MAX_IDEMPOTENCY_RECORDS);

        drop(connection);
        drop(store);
        let _ = std::fs::remove_file(database_path.with_extension("sqlite-shm"));
        let _ = std::fs::remove_file(database_path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(database_path);
    }

    #[test]
    fn removes_legacy_full_snapshot_events_and_compacts_database() {
        let database_path = std::env::temp_dir().join(format!(
            "todolist-migrate-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let workspace = Workspace {
            version: 3,
            projects: vec![Project {
                id: "project-1".into(),
                name: "TodoList".into(),
                color: "#1264f4".into(),
            }],
            tasks: vec![],
        };
        let payload = serde_json::to_string(&workspace).unwrap();
        let legacy_connection = Connection::open(&database_path).unwrap();
        legacy_connection
            .execute_batch(
                "CREATE TABLE workspace_snapshot (
                   id INTEGER PRIMARY KEY CHECK (id = 1),
                   version INTEGER NOT NULL,
                   payload_json TEXT NOT NULL,
                   updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE task_event (
                   change_seq INTEGER PRIMARY KEY AUTOINCREMENT,
                   workspace_version INTEGER NOT NULL,
                   actor TEXT NOT NULL,
                   action TEXT NOT NULL,
                   payload_json TEXT NOT NULL,
                   created_at INTEGER NOT NULL
                 );",
            )
            .unwrap();
        legacy_connection
            .execute(
                "INSERT INTO workspace_snapshot (id, version, payload_json, updated_at) VALUES (1, 3, ?1, 0)",
                params![payload],
            )
            .unwrap();
        legacy_connection
            .execute(
                "INSERT INTO task_event (workspace_version, actor, action, payload_json, created_at) VALUES (3, 'user', 'legacy', ?1, 0)",
                params!["x".repeat(1024 * 1024)],
            )
            .unwrap();
        drop(legacy_connection);

        let store = SqliteTaskStore::open(&database_path).expect("migrate store");
        let migrated_workspace = store.load_workspace().unwrap().unwrap();
        assert_eq!(migrated_workspace.version, workspace.version);
        assert_eq!(
            migrated_workspace.projects[0].name,
            workspace.projects[0].name
        );

        let migrated_connection = Connection::open(&database_path).unwrap();
        let legacy_table_exists: i64 = migrated_connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'task_event')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let schema_version: i64 = migrated_connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        let free_pages: i64 = migrated_connection
            .pragma_query_value(None, "freelist_count", |row| row.get(0))
            .unwrap();
        assert_eq!(legacy_table_exists, 0);
        assert_eq!(schema_version, 3);
        assert_eq!(free_pages, 0);

        drop(migrated_connection);
        drop(store);
        let _ = std::fs::remove_file(database_path.with_extension("sqlite-shm"));
        let _ = std::fs::remove_file(database_path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(database_path);
    }
}
