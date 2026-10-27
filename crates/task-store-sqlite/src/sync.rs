//! Bounded ID-only change journal. The current workspace remains the sole snapshot.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Serialize, Deserialize)]
struct Cursor {
    epoch: String,
    version: u64,
    target: u64,
    offset: usize,
}

pub struct ReceiptMutationResult {
    pub workspace: Workspace,
    pub receipt: Value,
}

fn remove_runtime_file_fields(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.remove("localPath");
            fields.remove("localPathStatus");
            fields.values_mut().for_each(remove_runtime_file_fields);
        }
        Value::Array(items) => items.iter_mut().for_each(remove_runtime_file_fields),
        _ => {}
    }
}

pub(crate) fn initialize(connection: &Connection) -> Result<(), String> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS workspace_sync_state (id INTEGER PRIMARY KEY CHECK(id=1),epoch TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS workspace_changes (version INTEGER PRIMARY KEY, previous_version INTEGER, changes_json TEXT NOT NULL, reset INTEGER NOT NULL);").map_err(|e|e.to_string())?;
    connection
        .execute(
            "INSERT OR IGNORE INTO workspace_sync_state(id,epoch) VALUES(1,?1)",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_receipt_table_never_stores_nested_runtime_file_paths() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteTaskStore::open(dir.path().join("receipt.sqlite")).unwrap();
        let w: Workspace =
            serde_json::from_value(json!({"version":1,"projects":[],"tasks":[]})).unwrap();
        store.save_workspace(&w).unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let result = store.mutate_with_receipt("review", "runtime-path", "same", |mut w| {
            w.version += 1;
            Ok((w, json!({"nested":{"files":[{"id":"file","localPath":root,"localPathStatus":"available"}]}})))
        }).unwrap();
        let stored: String = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT entity_id FROM mcp_idempotency WHERE operation='review'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!stored.contains("localPath"));
        assert!(!stored.contains(&root));
        assert_eq!(
            serde_json::from_str::<Value>(&stored).unwrap()["nested"]["files"][0],
            json!({"id":"file"})
        );
        assert_eq!(result.workspace.version, 2);
        let replay = store
            .mutate_with_receipt("review", "runtime-path", "same", |_| {
                panic!("must not mutate on retry")
            })
            .unwrap();
        assert_eq!(replay.receipt["replayed"], true);
        assert_eq!(replay.workspace.version, 2);
    }
    #[test]
    fn unknown_writer_and_paginated_snapshot_change_require_full_sync() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteTaskStore::open(dir.path().join("sync.sqlite")).unwrap();
        let w: Workspace = serde_json::from_value(
            json!({"version":1,"projects":[{"id":"p","name":"P","color":"#fff"}],"tasks":[]}),
        )
        .unwrap();
        store.save_workspace(&w).unwrap();
        let base = store.changes_since(None, 1).unwrap()["syncCursor"]
            .as_str()
            .unwrap()
            .to_string();
        store
            .mutate_workspace(|mut w| {
                w.version += 1;
                w.projects[0].name = "Renamed".into();
                w.projects.push(task_core::Project {
                    id: "q".into(),
                    name: "New".into(),
                    color: "#f00".into(),
                });
                Ok(w)
            })
            .unwrap();
        let first = store.changes_since(Some(&base), 1).unwrap();
        assert!(first["nextCursor"].is_string());
        assert_eq!(first["changes"][0]["kind"], "project");
        store
            .mutate_workspace(|mut w| {
                w.version += 1;
                w.projects[0].color = "#123456".into();
                Ok(w)
            })
            .unwrap();
        assert_eq!(
            store
                .changes_since(first["nextCursor"].as_str(), 1)
                .unwrap()["requiresFullSync"],
            true
        );
        let base = store.changes_since(None, 1).unwrap()["syncCursor"]
            .as_str()
            .unwrap()
            .to_string();
        let mut w = store.load_workspace().unwrap().unwrap();
        w.version += 1;
        store
            .connection()
            .unwrap()
            .execute(
                "UPDATE workspace_snapshot SET version=?1,payload_json=?2 WHERE id=1",
                params![w.version, serde_json::to_string(&w).unwrap()],
            )
            .unwrap();
        assert_eq!(
            store.changes_since(Some(&base), 10).unwrap()["requiresFullSync"],
            true
        );
    }
}
pub(crate) fn reset(connection: &Connection) -> Result<(), String> {
    connection
        .execute(
            "UPDATE workspace_sync_state SET epoch=?1 WHERE id=1",
            params![uuid::Uuid::new_v4().to_string()],
        )
        .map_err(|e| e.to_string())?;
    connection
        .execute("DELETE FROM workspace_changes", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub(crate) fn record(
    transaction: &Transaction<'_>,
    workspace: &Workspace,
    previous: Option<&Workspace>,
) -> Result<(), String> {
    let mut changes = Vec::new();
    for (index, task) in workspace.tasks.iter().enumerate() {
        let old = previous.and_then(|w| w.tasks.iter().enumerate().find(|(_, t)| t.id == task.id));
        if old.is_none_or(|(i, t)| {
            i != index || serde_json::to_value(t).unwrap() != serde_json::to_value(task).unwrap()
        }) {
            changes.push(json!({"kind":"task","id":task.id,"version":task.version,"deleted":false,"index":index,"workspaceVersion":workspace.version}));
        }
    }
    if let Some(old) = previous {
        for task in &old.tasks {
            if !workspace.tasks.iter().any(|t| t.id == task.id) {
                changes.push(json!({"kind":"task","id":task.id,"version":task.version,"deleted":true,"workspaceVersion":workspace.version}));
            }
        }
    }
    for project in &workspace.projects {
        if previous.is_none_or(|w| {
            !w.projects.iter().any(|p| {
                p.id == project.id
                    && serde_json::to_value(p).unwrap() == serde_json::to_value(project).unwrap()
            })
        }) {
            changes.push(json!({"kind":"project","id":project.id,"deleted":false,"workspaceVersion":workspace.version}));
        }
    }
    if let Some(old) = previous {
        for project in &old.projects {
            if !workspace.projects.iter().any(|p| p.id == project.id) {
                changes.push(json!({"kind":"project","id":project.id,"deleted":true,"workspaceVersion":workspace.version}));
            }
        }
    }
    let encoded = serde_json::to_string(&changes).map_err(|e| e.to_string())?;
    let requires_reset = previous.is_none() || changes.len() > 2000 || encoded.len() > 256 * 1024;
    transaction.execute("INSERT INTO workspace_changes(version,previous_version,changes_json,reset) VALUES(?1,?2,?3,?4)",params![workspace.version,previous.map(|w|w.version),if requires_reset{"[]"}else{&encoded},requires_reset]).map_err(|e|e.to_string())?;
    transaction.execute("DELETE FROM workspace_changes WHERE version IN (SELECT version FROM workspace_changes ORDER BY version DESC LIMIT -1 OFFSET 128)",[]).map_err(|e|e.to_string())?;
    loop {
        let bytes: i64 = transaction
            .query_row(
                "SELECT COALESCE(SUM(length(changes_json)),0) FROM workspace_changes",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if bytes <= 2 * 1024 * 1024 {
            break;
        }
        transaction.execute("DELETE FROM workspace_changes WHERE version=(SELECT MIN(version) FROM workspace_changes)",[]).map_err(|e|e.to_string())?;
    }
    Ok(())
}

impl SqliteTaskStore {
    /// A read transaction binds version, epoch and journal rows to one snapshot.
    pub fn changes_since(&self, since: Option<&str>, limit: usize) -> Result<Value, String> {
        if !(1..=200).contains(&limit) {
            return Err("changes limit must be 1 through 200".into());
        }
        let mut connection = self.connection()?;
        let tx = connection.transaction().map_err(|e| e.to_string())?;
        let version: u64 = tx
            .query_row(
                "SELECT version FROM workspace_snapshot WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let epoch: String = tx
            .query_row(
                "SELECT epoch FROM workspace_sync_state WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let baseline = || {
            serde_json::to_string(&Cursor {
                epoch: epoch.clone(),
                version,
                target: version,
                offset: 0,
            })
            .unwrap()
        };
        let Some(since) = since else {
            return Ok(
                json!({"workspaceVersion":version,"syncCursor":baseline(),"requiresFullSync":false,"changes":[],"nextCursor":null}),
            );
        };
        if since.len() > 2048 {
            return Err("invalid sync cursor".into());
        }
        let cursor: Cursor = serde_json::from_str(since).map_err(|_| "invalid sync cursor")?;
        let full = || json!({"workspaceVersion":version,"requiresFullSync":true,"reason":"restore, retention gap, unknown writer or changed paginated snapshot","syncCursor":baseline(),"changes":[],"nextCursor":null});
        if cursor.epoch != epoch
            || cursor.version > version
            || cursor.target < cursor.version
            || (cursor.offset > 0 && cursor.target != version)
        {
            return Ok(full());
        }
        let target = if cursor.offset == 0 {
            version
        } else {
            cursor.target
        };
        if cursor.version == target {
            return Ok(
                json!({"workspaceVersion":version,"requiresFullSync":false,"changes":[],"syncCursor":baseline(),"nextCursor":null}),
            );
        }
        let mut statement=tx.prepare("SELECT version,previous_version,changes_json,reset FROM workspace_changes WHERE version>?1 AND version<=?2 ORDER BY version").map_err(|e|e.to_string())?;
        let rows = statement
            .query_map(params![cursor.version, target], |r| {
                Ok((
                    r.get::<_, u64>(0)?,
                    r.get::<_, Option<u64>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, bool>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut previous = cursor.version;
        let mut events = Vec::<Value>::new();
        for row in rows {
            let (v, before, json, reset) = row.map_err(|e| e.to_string())?;
            if before != Some(previous) || reset {
                return Ok(full());
            }
            previous = v;
            events.extend(serde_json::from_str::<Vec<Value>>(&json).map_err(|e| e.to_string())?);
        }
        if previous != target || cursor.offset > events.len() {
            return Ok(full());
        }
        let end = (cursor.offset + limit).min(events.len());
        let next = if end < events.len() {
            Some(
                serde_json::to_string(&Cursor {
                    epoch: epoch.clone(),
                    version: cursor.version,
                    target,
                    offset: end,
                })
                .unwrap(),
            )
        } else {
            None
        };
        Ok(
            json!({"workspaceVersion":version,"requiresFullSync":false,"changes":events[cursor.offset..end],"nextCursor":next,"syncCursor":baseline(),"complete":next.is_none()}),
        )
    }

    /// Store a bounded exact receipt in the existing retry ledger, in the same
    /// transaction as the workspace write. A retry never replays append/add intent.
    pub fn mutate_with_receipt<F>(
        &self,
        operation: &str,
        request_id: &str,
        fingerprint: &str,
        mutate: F,
    ) -> Result<ReceiptMutationResult, String>
    where
        F: FnOnce(Workspace) -> Result<(Workspace, Value), String>,
    {
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let prior:Option<(String,String)>=tx.query_row("SELECT request_fingerprint,entity_id FROM mcp_idempotency WHERE operation=?1 AND request_id=?2",params![operation,request_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
        if let Some((old, result)) = prior {
            if old != fingerprint {
                return Err("idempotency conflict: request_id input changed".into());
            }
            let mut value: Value =
                serde_json::from_str(&result).map_err(|_| "invalid mutation receipt")?;
            remove_runtime_file_fields(&mut value);
            value["replayed"] = json!(true);
            let workspace =
                Self::load_from_connection(&tx)?.ok_or("workspace is not initialized")?;
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(ReceiptMutationResult {
                workspace,
                receipt: value,
            });
        }
        let current = Self::load_from_connection(&tx)?.ok_or("workspace is not initialized")?;
        let (mut updated, mut result) = mutate(current.clone())?;
        updated.trim_activity();
        Self::reconcile_completion_times(&mut updated, Some(&current));
        updated.validate()?;
        updated.validate_completion_changes(&current)?;
        result["workspaceVersion"] = json!(updated.version);
        result["replayed"] = json!(false);
        remove_runtime_file_fields(&mut result);
        let receipt = serde_json::to_string(&result).map_err(|e| e.to_string())?;
        if receipt.len() > 32768 {
            return Err("mutation receipt exceeds 32 KiB; reduce the number of edits".into());
        }
        Self::persist_transaction(&tx, &updated, Some(&current))?;
        tx.execute("INSERT INTO mcp_idempotency(operation,request_id,request_fingerprint,entity_id,created_at) VALUES(?1,?2,?3,?4,?5)",params![operation,request_id,fingerprint,receipt,chrono::Utc::now().timestamp()]).map_err(|e|e.to_string())?;
        tx.execute("DELETE FROM mcp_idempotency WHERE rowid IN (SELECT rowid FROM mcp_idempotency ORDER BY rowid DESC LIMIT -1 OFFSET ?1)",params![MAX_IDEMPOTENCY_RECORDS]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(ReceiptMutationResult {
            workspace: updated,
            receipt: result,
        })
    }
}
