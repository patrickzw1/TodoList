use super::*;
use serde_json::{json, Value};

fn call(s: &TodoMcpServer, name: &str, v: Value) -> CallToolResult {
    macro_rules! arg {
        () => {
            Parameters(serde_json::from_value(v).unwrap())
        };
    }
    match name {
        "list_tasks" => s.list_tasks(arg!()),
        "list_projects" => s.list_projects(arg!()),
        "get_task" => s.get_task(arg!()),
        "task_stats" => s.task_stats(arg!()),
        "update_task" => s.update_task(arg!()),
        "batch_update" => s.batch_update(arg!()),
        "add_task_attachment" => s.add_task_attachment(arg!()),
        "add_task_image" => s.add_task_image(arg!()),
        "list_task_files" => s.list_task_files(arg!()),
        "remove_task_files" => s.remove_task_files(arg!()),
        "read_task_file" => s.read_task_file(arg!()),
        "search_file_content" => s.search_file_content(arg!()),
        "workspace_info" => s.workspace_info(arg!()),
        _ => panic!("unknown test tool"),
    }
}
fn good(r: CallToolResult) -> Value {
    assert_ne!(r.is_error, Some(true), "{:?}", r.content);
    serde_json::from_str(&r.content[0].as_text().unwrap().text).unwrap()
}
fn bad(r: CallToolResult) -> String {
    assert_eq!(r.is_error, Some(true), "{:?}", r.content);
    r.content[0].as_text().unwrap().text.clone()
}
fn setup(count: usize) -> (TodoMcpServer, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteTaskStore::open(dir.path().join("fixture.sqlite")).unwrap();
    let mut tasks = Vec::new();
    for i in 0..count {
        tasks.push(json!({"id":format!("t{i}"),"projectId":"p","title":format!("任务{i}"),"description":"synthetic ".repeat(1000),"status":"todo","priority":"medium","dueLabel":"未安排","dueDate":"9999-12-31","tags":[],"source":"手动创建","archived":false,"pinned":false,"version":1,"subtasks":[],"acceptanceCriteria":[],"attachments":[],"images":[],"dependencies":[],"activity":[]}));
    }
    let w:Workspace=serde_json::from_value(json!({"version":1,"projects":[{"id":"p","name":"重复项目","color":"#123456"},{"id":"q","name":"重复项目","color":"#abcdef"}],"tasks":tasks})).unwrap();
    store.save_workspace(&w).unwrap();
    (TodoMcpServer::new(store), dir)
}
fn version(s: &TodoMcpServer, id: &str) -> u64 {
    s.store
        .load_workspace()
        .unwrap()
        .unwrap()
        .tasks
        .into_iter()
        .find(|t| t.id == id)
        .unwrap()
        .version
}
fn attach(s: &TodoMcpServer, id: &str, name: &str, body: &str, key: &str) -> Value {
    good(call(
        s,
        "add_task_attachment",
        json!({"task_id":id,"expected_version":version(s,id),"text":{"name":name,"body":body},"request_id":key}),
    ))
}

#[test]
fn compact_queries_projection_pagination_and_ambiguity() {
    let (s, _root) = setup(32);
    let first = good(call(&s, "list_tasks", json!({})));
    assert_eq!(first["tasks"].as_array().unwrap().len(), 15);
    assert!(first["tasks"][0].get("description").is_none());
    assert!(first["tasks"][0].get("subtasks").is_none());
    assert_eq!(first["tasks"][0]["counts"]["attachments"], 0);
    let cursor = first["nextCursor"].clone();
    let second = good(call(&s, "list_tasks", json!({"cursor":cursor})));
    assert_eq!(second["tasks"][0]["id"], "t15");
    assert!(bad(call(
        &s,
        "list_tasks",
        json!({"cursor":cursor,"projection":"full"})
    ))
    .contains("cursor"));
    let projects = good(call(
        &s,
        "list_projects",
        json!({"query":" 重 复项目 ","match_mode":"exact"}),
    ));
    assert_eq!(projects["totalCount"], 2);
    assert_eq!(projects["ambiguous"], true);
    let ids = good(call(
        &s,
        "list_tasks",
        json!({"projection":"ids","limit":2}),
    ));
    assert_eq!(ids["tasks"][0].as_object().unwrap().len(), 2);
    let full = good(call(
        &s,
        "list_tasks",
        json!({"projection":"full","limit":1}),
    ));
    assert!(full["tasks"][0].get("description").is_some());
    assert!(full["tasks"][0].get("activity").is_none());
    good(call(
        &s,
        "update_task",
        json!({"task_id":"t0","expected_version":1,"title":"changed"}),
    ));
    assert!(bad(call(&s, "list_tasks", json!({"cursor":cursor}))).contains("stale"));
}
#[test]
fn chinese_multifield_search_and_statistics_share_filter() {
    let (s, _root) = setup(8);
    s.store
        .mutate_workspace(|mut w| {
            w.version += 1;
            w.tasks[0].title = "权限设计".into();
            w.tasks[1].description = "开始 验收 目 标 结束".into();
            w.tasks[2].subtasks.push(Subtask {
                id: "i".into(),
                title: "验收目标".into(),
                completed: false,
            });
            w.tasks[3].acceptance_criteria.push(AcceptanceCriterion {
                id: "c".into(),
                title: "验收目标".into(),
                completed: false,
            });
            w.tasks[4].dependencies.push("验收目标".into());
            w.tasks[5].tags.push("验收目标".into());
            w.tasks[6].attachments.push(ManagedFile {
                id: "missing".into(),
                original_name: "验收目标.md".into(),
                media_type: "text/plain".into(),
                size: 0,
                storage_key: "attachments/missing.md".into(),
                added_at: "now".into(),
            });
            w.tasks[7].archived = true;
            Ok(w)
        })
        .unwrap();
    for (field, id) in [
        ("description", "t1"),
        ("subtasks", "t2"),
        ("acceptance_criteria", "t3"),
        ("dependencies", "t4"),
        ("tags", "t5"),
        ("files", "t6"),
    ] {
        let result = good(call(
            &s,
            "list_tasks",
            json!({"filter":{"query":"验收目标","fields":[field]}}),
        ));
        assert_eq!(result["tasks"][0]["id"], id);
        assert_eq!(result["tasks"][0]["matches"][0]["field"], field);
        assert!(
            result["tasks"][0]["matches"][0]["excerpt"]
                .as_str()
                .unwrap()
                .chars()
                .count()
                <= 160
        );
    }
    let fuzzy = good(call(
        &s,
        "list_tasks",
        json!({"filter":{"query":"权限设汁","fields":["title"],"match_mode":"fuzzy"}}),
    ));
    assert_eq!(fuzzy["tasks"][0]["id"], "t0");
    let archived = good(call(
        &s,
        "list_tasks",
        json!({"filter":{"archived":"archived"}}),
    ));
    assert_eq!(archived["tasks"][0]["id"], "t7");
    let filter = json!({"query":"验收目标","has_attachments":true});
    let listed = good(call(&s, "list_tasks", json!({"filter":filter})));
    let stats = good(call(
        &s,
        "task_stats",
        json!({"filter":filter,"today":"2026-10-27"}),
    ));
    assert_eq!(stats["total"], listed["totalCount"]);
    assert!(stats.get("tasks").is_none());
}
#[test]
fn dates_and_sections_are_explicit_and_bounded() {
    let (s, _root) = setup(3);
    s.store
        .mutate_workspace(|mut w| {
            w.version += 1;
            w.tasks[0].due_date = "2026-10-26".into();
            w.tasks[1].status = TaskStatus::Done;
            w.tasks[1].due_date = "2026-10-27".into();
            w.tasks[1].completed_at = Some("2026-10-26T23:30:00Z".into());
            w.tasks[2].description = "甲乙丙丁戊己".into();
            Ok(w)
        })
        .unwrap();
    let stats = good(call(
        &s,
        "task_stats",
        json!({"filter":{"utc_offset_minutes":480},"today":"2026-10-27"}),
    ));
    assert_eq!(stats["overdue"], 1);
    assert_eq!(stats["dueToday"], 1);
    assert_eq!(stats["completedToday"], 1);
    let done = good(call(
        &s,
        "list_tasks",
        json!({"filter":{"completed_from":"2026-10-27","completed_to":"2026-10-27","utc_offset_minutes":480}}),
    ));
    assert_eq!(done["tasks"][0]["id"], "t1");
    let part = good(call(
        &s,
        "get_task",
        json!({"task_id":"t2","sections":["description"],"description_start":2,"description_limit":2}),
    ));
    assert_eq!(part["task"]["description"], "丙丁");
    assert_eq!(part["task"]["descriptionRange"]["nextStart"], 4);
    assert!(part["task"].get("attachments").is_none());
    let many = good(call(
        &s,
        "get_task",
        json!({"task_ids":["t0","missing"],"fields":["status"]}),
    ));
    assert_eq!(many["missingIds"][0], "missing");
    assert_eq!(many["tasks"][0].as_object().unwrap().len(), 3);
}
#[test]
fn local_edits_are_exact_idempotent_and_keep_item_ids() {
    let (s, _root) = setup(1);
    let input = json!({"task_id":"t0","expected_version":1,"request_id":"append-1","description_edit":{"action":"append","text":"\n尾部"},"subtask_edits":[{"action":"add","item_id":"stable","title":"分项"}]});
    let first = good(call(&s, "update_task", input.clone()));
    let repeat = good(call(&s, "update_task", input.clone()));
    assert_eq!(repeat["replayed"], true);
    assert_eq!(first["task"]["version"], repeat["task"]["version"]);
    assert_eq!(first["task"].as_object().unwrap().len(), 2);
    let w = s.store.load_workspace().unwrap().unwrap();
    assert_eq!(w.tasks[0].description.matches("尾部").count(), 1);
    assert_eq!(w.tasks[0].subtasks[0].id, "stable");
    let mut altered = input;
    altered["description_edit"]["text"] = json!("changed");
    assert!(bad(call(&s, "update_task", altered)).contains("idempotency"));
    good(call(
        &s,
        "update_task",
        json!({"task_id":"t0","expected_version":2,"description":"aaa"}),
    ));
    for old in ["not present", "aa"] {
        assert!(bad(call(&s,"update_task",json!({"task_id":"t0","expected_version":3,"request_id":format!("replace-{old}"),"description_edit":{"action":"replace","old":old,"text":"new"}}))).contains("exactly once"));
    }
    good(call(
        &s,
        "update_task",
        json!({"task_id":"t0","expected_version":3,"request_id":"check","subtask_edits":[{"action":"set_completed","item_id":"stable","completed":true}]}),
    ));
    let w = s.store.load_workspace().unwrap().unwrap();
    assert!(w.tasks[0].subtasks[0].completed);
    assert_eq!(w.tasks[0].status, TaskStatus::Todo);
}

#[test]
fn review_full_retry_uses_current_associations_and_file_availability() {
    let (s, _root) = setup(1);
    let attachment = attach(&s, "t0", "review.txt", "file content", "review-file");
    let image = good(call(
        &s,
        "add_task_image",
        json!({"task_id":"t0","expected_version":version(&s,"t0"),"source_path":Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.png"),"request_id":"review-image"}),
    ));
    let whole = json!({"task_id":"t0","expected_version":version(&s,"t0"),"title":"whole edit","request_id":"review-whole","response":"full"});
    let first = good(call(&s, "update_task", whole.clone()));
    assert_eq!(
        first["task"]["attachments"][0]["localPathStatus"],
        "available"
    );
    let applied = first["appliedTask"].clone();
    let local = json!({"task_id":"t0","expected_version":version(&s,"t0"),"description_edit":{"action":"append","text":"\nreview-once"},"request_id":"review-local","response":"full"});
    let patched = good(call(&s, "update_task", local.clone()));
    assert_eq!(patched["task"]["images"][0]["localPathStatus"], "available");
    let applied_patch = patched["appliedTask"].clone();
    fs::remove_file(attachment["file"]["localPath"].as_str().unwrap()).unwrap();
    let missing = good(call(&s, "update_task", whole.clone()));
    assert_eq!(missing["replayed"], true);
    assert_eq!(missing["appliedTask"], applied);
    assert_eq!(missing["task"]["version"], patched["task"]["version"]);
    assert_eq!(
        missing["task"]["attachments"][0]["localPathStatus"],
        "missing"
    );
    assert!(missing["task"]["attachments"][0]["localPath"].is_null());
    good(call(
        &s,
        "remove_task_files",
        json!({"task_id":"t0","expected_version":version(&s,"t0"),"file_ids":[image["file"]["id"]],"request_id":"review-unlink"}),
    ));
    let unlinked = good(call(&s, "update_task", local.clone()));
    assert_eq!(unlinked["replayed"], true);
    assert_eq!(unlinked["appliedTask"], applied_patch);
    assert!(unlinked["task"]["images"].as_array().unwrap().is_empty());
    assert_eq!(
        unlinked["task"]["description"]
            .as_str()
            .unwrap()
            .matches("review-once")
            .count(),
        1
    );
    let before = version(&s, "t0");
    good(call(&s, "update_task", local.clone()));
    assert_eq!(version(&s, "t0"), before);
    s.store
        .mutate_workspace(|mut w| {
            w.version += 1;
            w.tasks.clear();
            Ok(w)
        })
        .unwrap();
    let removed = good(call(&s, "update_task", local));
    assert_eq!(removed["appliedTask"], applied_patch);
    assert!(removed["task"].is_null());
}

#[test]
fn review_search_reasons_identify_same_title_items_without_expanding_projection() {
    let (s, _root) = setup(1);
    s.store
        .mutate_workspace(|mut w| {
            w.version += 1;
            let t = &mut w.tasks[0];
            for (id, completed) in [("sub-a", false), ("sub-b", true)] {
                t.subtasks.push(Subtask {
                    id: id.into(),
                    title: "相同标题".into(),
                    completed,
                });
            }
            for (id, completed) in [("criterion-a", true), ("criterion-b", false)] {
                t.acceptance_criteria.push(AcceptanceCriterion {
                    id: id.into(),
                    title: "相同标题".into(),
                    completed,
                });
            }
            for id in ["file-a", "file-b"] {
                t.attachments.push(ManagedFile {
                    id: id.into(),
                    original_name: "相同标题.txt".into(),
                    media_type: "text/plain".into(),
                    size: 0,
                    storage_key: format!("attachments/{id}.txt"),
                    added_at: "now".into(),
                });
            }
            t.dependencies.push("相同标题".into());
            Ok(w)
        })
        .unwrap();
    for projection in ["compact", "ids"] {
        let result = good(call(
            &s,
            "list_tasks",
            json!({"projection":projection,"filter":{"query":"相同标题","fields":["subtasks","acceptance_criteria"]}}),
        ));
        let task = &result["tasks"][0];
        assert!(task.get("description").is_none());
        assert!(task.get("subtasks").is_none());
        assert!(task.get("acceptanceCriteria").is_none());
        let reasons = task["matches"].as_array().unwrap();
        assert_eq!(reasons.len(), 4);
        assert_eq!(
            reasons
                .iter()
                .map(|r| r["itemId"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["sub-a", "sub-b", "criterion-a", "criterion-b"]
        );
        assert_eq!(
            reasons
                .iter()
                .map(|r| r["completed"].as_bool().unwrap())
                .collect::<Vec<_>>(),
            [false, true, true, false]
        );
        assert!(reasons
            .iter()
            .all(|r| r["excerpt"].as_str().unwrap().chars().count() <= 160));
        if projection == "ids" {
            assert_eq!(task.as_object().unwrap().len(), 3);
        }
    }
    let files = good(call(
        &s,
        "list_tasks",
        json!({"projection":"ids","filter":{"query":"相同标题","fields":["files"]}}),
    ));
    assert_eq!(files["tasks"][0]["matches"][0]["fileId"], "file-a");
    assert_eq!(files["tasks"][0]["matches"][1]["fileId"], "file-b");
    assert!(files["tasks"][0].get("attachments").is_none());
    let text = good(call(
        &s,
        "list_tasks",
        json!({"projection":"ids","filter":{"query":"相同标题","fields":["dependencies"]}}),
    ));
    let reason = &text["tasks"][0]["matches"][0];
    assert!(reason.get("itemId").is_none());
    assert!(reason.get("fileId").is_none());
    assert!(reason.get("completed").is_none());
    let bounded = good(call(
        &s,
        "list_tasks",
        json!({"filter":{"query":"相同标题"}}),
    ));
    assert_eq!(bounded["tasks"][0]["matches"].as_array().unwrap().len(), 4);
}

#[cfg(windows)]
#[test]
fn review_remove_reports_pending_cleanup_while_file_is_held_open() {
    use std::os::windows::fs::OpenOptionsExt;
    let (s, _root) = setup(1);
    let attachment = attach(&s, "t0", "locked.txt", "held open", "locked-file");
    let path = attachment["file"]["localPath"].as_str().unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .unwrap();
    let args = json!({"task_id":"t0","expected_version":version(&s,"t0"),"file_ids":[attachment["file"]["id"]],"request_id":"locked-remove"});
    let first = good(call(&s, "remove_task_files", args.clone()));
    assert_eq!(first["cleanupPending"], true);
    assert!(s.store.managed_file_cleanup_pending().unwrap());
    assert!(Path::new(path).exists());
    assert!(s.store.load_workspace().unwrap().unwrap().tasks[0]
        .attachments
        .is_empty());
    let retry = good(call(&s, "remove_task_files", args.clone()));
    assert_eq!(retry["replayed"], true);
    assert_eq!(retry["cleanupPending"], true);
    assert_eq!(retry["task"]["version"], first["task"]["version"]);
    drop(held);
    let cleaned = good(call(&s, "remove_task_files", args));
    assert_eq!(cleaned["replayed"], true);
    assert_eq!(cleaned["cleanupPending"], false);
    assert!(!s.store.managed_file_cleanup_pending().unwrap());
    assert!(!Path::new(path).exists());
}
#[test]
fn bulk_changes_are_atomic_archive_safe_and_preserve_completed_time() {
    let (s, _root) = setup(2);
    good(call(
        &s,
        "update_task",
        json!({"task_id":"t0","expected_version":1,"status":"done"}),
    ));
    let before = s.store.load_workspace().unwrap().unwrap();
    let time = before.tasks[0].completed_at.clone();
    let bad_input = json!({"request_id":"bulk-conflict","tasks":[{"task_id":"t0","expected_version":2,"archived":true},{"task_id":"t1","expected_version":999,"priority":"high"}]});
    assert!(bad(call(&s, "batch_update", bad_input)).contains("version_conflict"));
    assert!(!s.store.load_workspace().unwrap().unwrap().tasks[0].archived);
    let input = json!({"request_id":"bulk-1","tasks":[{"task_id":"t0","expected_version":2,"archived":true,"project_id":"q","tags_add":["归档"]},{"task_id":"t1","expected_version":1,"priority":"high","due_date":"2026-10-28"}]});
    let result = good(call(&s, "batch_update", input.clone()));
    assert_eq!(result["atomic"], true);
    assert_eq!(good(call(&s, "batch_update", input))["replayed"], true);
    let w = s.store.load_workspace().unwrap().unwrap();
    assert_eq!(w.tasks[0].completed_at, time);
    assert!(w.tasks[0].archived);
    assert!(!w.tasks[0].pinned);
    assert_eq!(w.tasks[0].project_id, "q");
    assert!(bad(call(
        &s,
        "update_task",
        json!({"task_id":"t0","expected_version":3,"status":"todo"})
    ))
    .contains("reopen"));
    good(call(
        &s,
        "batch_update",
        json!({"request_id":"project","expected_workspace_version":w.version,"projects":[{"project_id":"p","color":"#F00"}]}),
    ));
}
#[test]
fn criterion_local_edits_enforce_completion_and_preserve_concurrent_items() {
    let (s, _root) = setup(1);
    good(call(
        &s,
        "update_task",
        json!({"task_id":"t0","expected_version":1,"request_id":"criterion-add","criterion_edits":[{"action":"add","item_id":"criterion-a","title":"先验收"}]}),
    ));
    let blocked = call(
        &s,
        "update_task",
        json!({"task_id":"t0","expected_version":2,"request_id":"blocked","status":"done"}),
    );
    assert!(bad(blocked).contains("acceptance"));
    assert_eq!(version(&s, "t0"), 2);
    good(call(
        &s,
        "update_task",
        json!({"task_id":"t0","expected_version":2,"request_id":"criterion-confirm","criterion_edits":[{"action":"set_completed","item_id":"criterion-a","completed":true}],"status":"done"}),
    ));
    assert!(s.store.load_workspace().unwrap().unwrap().tasks[0]
        .completed_at
        .is_some());
    good(call(
        &s,
        "update_task",
        json!({"task_id":"t0","expected_version":3,"request_id":"reopen","status":"todo","allow_reopen_completed":true}),
    ));
    good(call(
        &s,
        "update_task",
        json!({"task_id":"t0","expected_version":4,"request_id":"other-add","criterion_edits":[{"action":"add","item_id":"criterion-b","title":"另一个客户端新增"}]}),
    ));
    assert!(bad(call(&s, "update_task", json!({"task_id":"t0","expected_version":4,"request_id":"stale-rename","criterion_edits":[{"action":"rename","item_id":"criterion-a","title":"改名"}]}))).contains("version_conflict"));
    good(call(
        &s,
        "update_task",
        json!({"task_id":"t0","expected_version":5,"request_id":"rename-latest","criterion_edits":[{"action":"rename","item_id":"criterion-a","title":"改名"}]}),
    ));
    let workspace = s.store.load_workspace().unwrap().unwrap();
    let task = &workspace.tasks[0];
    assert_eq!(task.acceptance_criteria.len(), 2);
    assert!(task.acceptance_criteria[0].completed);
    assert_eq!(task.acceptance_criteria[1].id, "criterion-b");
    assert_eq!(task.completed_at, None);
}
#[test]
fn file_text_import_read_cursor_remove_and_original_are_safe() {
    let (s, root) = setup(2);
    let imported = attach(&s, "t0", "笔记.md", "第一行\n第二行\n第三行", "text");
    let id = imported["file"]["id"].as_str().unwrap();
    assert!(imported["file"]["localPath"].is_string());
    let req = json!({"task_id":"t0","expected_version":1,"request_id":"text","text":{"name":"笔记.md","body":"第一行\n第二行\n第三行"}});
    assert_eq!(good(call(&s, "add_task_attachment", req))["replayed"], true);
    let args = json!({"task_id":"t0","file_id":id,"limit":1});
    let first = good(call(&s, "read_task_file", args.clone()));
    assert_eq!(first["units"][0]["text"], "第一行\n");
    let mut next = args.clone();
    next["cursor"] = first["nextCursor"].clone();
    assert_eq!(
        good(call(&s, "read_task_file", next.clone()))["units"][0]["text"],
        "第二行\n"
    );
    assert!(bad(call(
        &s,
        "read_task_file",
        json!({"task_id":"t1","file_id":id})
    ))
    .contains("associated"));
    fs::write(
        imported["file"]["localPath"].as_str().unwrap(),
        "changed bytes",
    )
    .unwrap();
    assert!(bad(call(&s, "read_task_file", next)).contains("stale"));
    let remove = json!({"task_id":"t0","expected_version":2,"file_ids":[id],"request_id":"remove"});
    good(call(&s, "remove_task_files", remove.clone()));
    assert_eq!(
        good(call(&s, "remove_task_files", remove))["replayed"],
        true
    );
    assert!(!Path::new(imported["file"]["localPath"].as_str().unwrap()).exists());
    let source = root.path().join("source.txt");
    fs::write(&source, "original stays").unwrap();
    let out = good(call(
        &s,
        "add_task_attachment",
        json!({"task_id":"t1","expected_version":1,"source_path":source,"request_id":"source"}),
    ));
    good(call(
        &s,
        "remove_task_files",
        json!({"task_id":"t1","expected_version":2,"file_ids":[out["file"]["id"]],"request_id":"remove-source"}),
    ));
    assert_eq!(fs::read_to_string(source).unwrap(), "original stays");
    assert!(bad(call(&s,"add_task_attachment",json!({"task_id":"t0","expected_version":3,"request_id":"unsafe","text":{"name":"../escape.txt","body":"x"}}))).contains("filename"));
}
#[test]
fn real_formats_and_image_use_only_explicit_associated_content() {
    let (s, _root) = setup(1);
    for ext in ["pdf", "docx", "pptx", "xlsx", "png"] {
        let source =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/sample.{ext}"));
        let added = good(call(
            &s,
            if ext == "png" {
                "add_task_image"
            } else {
                "add_task_attachment"
            },
            json!({"task_id":"t0","expected_version":version(&s,"t0"),"source_path":source,"request_id":ext}),
        ));
        let result = call(
            &s,
            "read_task_file",
            json!({"task_id":"t0","file_id":added["file"]["id"]}),
        );
        assert_ne!(result.is_error, Some(true));
        let meta: Value = serde_json::from_str(&result.content[0].as_text().unwrap().text).unwrap();
        assert_eq!(
            meta["status"],
            if ext == "png" { "requires_ocr" } else { "ok" }
        );
        if ext == "png" {
            assert!(matches!(&result.content[1], ContentBlock::Image(_)));
        } else {
            assert!(!meta["units"].as_array().unwrap().is_empty());
        }
    }
    let list = good(call(&s, "list_tasks", json!({})));
    assert!(list["tasks"][0].get("attachments").is_none());
    let files = good(call(
        &s,
        "list_task_files",
        json!({"task_id":"t0","query":"SAMPLE.PDF"}),
    ));
    assert_eq!(files["totalCount"], 1);
}
#[test]
fn content_search_pages_never_claim_a_full_negative_before_finishing() {
    let (s, _root) = setup(1);
    let body = (0..210)
        .map(|i| {
            if i == 205 {
                "目标命中".into()
            } else {
                format!("row {i}")
            }
        })
        .collect::<Vec<String>>()
        .join("\n");
    attach(&s, "t0", "long.txt", &body, "long");
    let mut args = json!({"task_ids":["t0"],"query":"目标命中"});
    let mut hits = 0;
    let mut pages = 0;
    loop {
        let page = good(call(&s, "search_file_content", args.clone()));
        pages += 1;
        hits += page["hits"].as_array().unwrap().len();
        if page["nextCursor"].is_null() {
            assert_eq!(page["fullySearched"], true);
            break;
        }
        assert_eq!(page["fullySearched"], false);
        assert_eq!(page["partial"], true);
        args["cursor"] = page["nextCursor"].clone();
        assert!(pages < 10);
    }
    assert!(pages >= 3);
    assert_eq!(hits, 1);
    attach(
        &s,
        "t0",
        "bad.json",
        "bad UTF8 is separately tested; Unicode 有界",
        "other",
    );
    let unsafe_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scanned.pdf");
    let file = good(call(
        &s,
        "add_task_attachment",
        json!({"task_id":"t0","expected_version":version(&s,"t0"),"source_path":unsafe_file,"request_id":"scan"}),
    ));
    let read = good(call(
        &s,
        "read_task_file",
        json!({"task_id":"t0","file_id":file["file"]["id"]}),
    ));
    assert_eq!(read["status"], "requires_ocr");
    let page = good(call(
        &s,
        "search_file_content",
        json!({"task_ids":["t0"],"query":"absent","file_name":"scanned"}),
    ));
    assert_eq!(page["scanComplete"], true);
    assert_eq!(page["fullySearched"], false);
    assert_eq!(page["partial"], true);
}
#[test]
fn long_chinese_keyword_survives_more_than_two_small_content_pages() {
    let (s, _root) = setup(1);
    let query = "甲".repeat(127) + "乙";
    let file = attach(&s, "t0", "boundary.txt", &query, "boundary");
    let mut args = json!({"task_id":"t0","file_id":file["file"]["id"],"query":query,"max_bytes":128,"limit":1});
    let mut found = false;
    for _ in 0..8 {
        let page = good(call(&s, "read_task_file", args.clone()));
        found |= !page["units"].as_array().unwrap().is_empty();
        if page["nextCursor"].is_null() {
            break;
        }
        args["cursor"] = page["nextCursor"].clone();
    }
    assert!(found);
}
#[test]
fn changes_are_id_only_and_expire_on_restore_retention_or_unknown_writer() {
    let (s, _root) = setup(2);
    let baseline = good(call(&s, "workspace_info", json!({})));
    let cursor = baseline["syncCursor"].clone();
    good(call(
        &s,
        "update_task",
        json!({"task_id":"t0","expected_version":1,"title":"updated"}),
    ));
    let delta = good(call(&s, "workspace_info", json!({"since":cursor})));
    assert_eq!(delta["requiresFullSync"], false);
    assert_eq!(delta["changes"][0]["id"], "t0");
    assert!(delta["changes"][0].get("description").is_none());
    let before_delete = delta["syncCursor"].clone();
    s.store
        .mutate_workspace(|mut w| {
            w.version += 1;
            w.tasks.retain(|t| t.id != "t1");
            Ok(w)
        })
        .unwrap();
    let deleted = good(call(&s, "workspace_info", json!({"since":before_delete})));
    assert_eq!(deleted["requiresFullSync"], false);
    assert_eq!(deleted["changes"][0]["id"], "t1");
    assert_eq!(deleted["changes"][0]["deleted"], true);
    assert_eq!(deleted["changes"][0].as_object().unwrap().len(), 5);
    let mut w = s.store.load_workspace().unwrap().unwrap();
    w.version += 1;
    s.store.restore_workspace(&w).unwrap();
    assert_eq!(
        good(call(&s, "workspace_info", json!({"since":cursor})))["requiresFullSync"],
        true
    );
    let fresh = good(call(&s, "workspace_info", json!({})));
    let old = fresh["syncCursor"].clone();
    for i in 0..130 {
        s.store
            .mutate_workspace(|mut w| {
                w.version += 1;
                w.tasks[0].version += 1;
                w.tasks[0].title = format!("revision {i}");
                Ok(w)
            })
            .unwrap();
    }
    assert_eq!(
        good(call(&s, "workspace_info", json!({"since":old})))["requiresFullSync"],
        true
    );
}
#[test]
fn synthetic_response_byte_comparison() {
    let (s, _root) = setup(60);
    let full50 = good(call(
        &s,
        "list_tasks",
        json!({"projection":"full","limit":50}),
    ));
    let full15 = good(call(
        &s,
        "list_tasks",
        json!({"projection":"full","limit":15}),
    ));
    let compact15 = good(call(&s, "list_tasks", json!({"limit":15})));
    let search = good(call(
        &s,
        "list_tasks",
        json!({"filter":{"query":"任务37","fields":["title"],"match_mode":"exact"}}),
    ));
    let write = good(call(
        &s,
        "update_task",
        json!({"task_id":"t37","expected_version":1,"request_id":"small","description_edit":{"action":"append","text":"\nappend"}}),
    ));
    let bytes = |v: &Value| serde_json::to_vec(v).unwrap().len();
    let metrics = json!({"unit":"UTF-8 response JSON bytes; not token counts","full50":bytes(&full50),"full15":bytes(&full15),"compact15":bytes(&compact15),"exactSearch1":bytes(&search),"appendResponse":bytes(&write),"toolDefinitions":serde_json::to_vec(&TodoMcpServer::tool_router().list_all()).unwrap().len()});
    println!("MCP_BYTE_EVIDENCE={metrics}");
    assert!(bytes(&compact15) < bytes(&full15));
    assert!(bytes(&write) < 1000);
}
