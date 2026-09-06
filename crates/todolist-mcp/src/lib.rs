use chrono::Local;
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_handler, tool_router, ServerHandler,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use task_core::{AcceptanceCriterion, ActivityItem, Priority, Project, Subtask, Task, TaskStatus};
use task_store_sqlite::{IdempotentMutationResult, SqliteTaskStore};
use uuid::Uuid;

const DEFAULT_TASK_PAGE_SIZE: u32 = 50;
const MAX_TASK_PAGE_SIZE: u32 = 100;
const MAX_REQUEST_ID_LENGTH: usize = 128;

#[derive(Debug, Clone)]
pub struct TodoMcpServer {
    store: SqliteTaskStore,
}

impl TodoMcpServer {
    pub fn new(store: SqliteTaskStore) -> Self {
        Self { store }
    }

    fn success(value: serde_json::Value) -> CallToolResult {
        CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&value).expect("serialize tool result"),
        )])
    }

    fn error(message: impl Into<String>) -> CallToolResult {
        CallToolResult::error(vec![ContentBlock::text(message.into())])
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListTasksInput {
    #[schemars(description = "Optional project id to filter by")]
    pub project_id: Option<String>,
    #[schemars(description = "Optional status: todo, in_progress, blocked, or done")]
    pub status: Option<TaskStatusInput>,
    #[schemars(description = "Whether completed tasks are included; defaults to true")]
    pub include_done: Option<bool>,
    #[schemars(description = "Whether archived tasks are included; defaults to false")]
    pub include_archived: Option<bool>,
    #[schemars(description = "Maximum tasks to return; defaults to 50 and cannot exceed 100")]
    pub limit: Option<u32>,
    #[schemars(description = "Opaque nextCursor returned by the previous list_tasks call")]
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetTaskInput {
    #[schemars(description = "Task id")]
    pub task_id: String,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct CreateProjectInput {
    #[schemars(
        description = "Stable key for safely retrying this creation request; maximum 128 characters"
    )]
    pub request_id: Option<String>,
    #[schemars(description = "Unique project name")]
    pub name: String,
    #[schemars(description = "Optional CSS hex color; defaults to #1264f4")]
    pub color: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct CreateTaskInput {
    #[schemars(
        description = "Stable key for safely retrying this creation request; maximum 128 characters"
    )]
    pub request_id: Option<String>,
    #[schemars(description = "Existing TodoList project id")]
    pub project_id: String,
    #[schemars(description = "Short task title")]
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<PriorityInput>,
    #[schemars(description = "ISO date YYYY-MM-DD; omit for an unscheduled task")]
    pub due_date: Option<String>,
    pub tags: Option<Vec<String>>,
    pub subtasks: Option<Vec<SubtaskInput>>,
    pub acceptance_criteria: Option<Vec<AcceptanceCriterionInput>>,
    #[schemars(description = "Human-readable dependency descriptions")]
    pub dependencies: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateTaskInput {
    #[schemars(description = "Task id")]
    pub task_id: String,
    #[schemars(description = "Latest task version returned by get_task or list_tasks")]
    pub expected_version: u64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<TaskStatusInput>,
    pub priority: Option<PriorityInput>,
    #[schemars(description = "ISO date YYYY-MM-DD")]
    pub due_date: Option<String>,
    pub tags: Option<Vec<String>>,
    #[schemars(description = "When present, replaces the subtask list")]
    pub subtasks: Option<Vec<SubtaskInput>>,
    #[schemars(description = "When present, replaces the acceptance criteria")]
    pub acceptance_criteria: Option<Vec<AcceptanceCriterionInput>>,
    #[schemars(description = "When present, replaces the dependency list")]
    pub dependencies: Option<Vec<String>>,
    #[schemars(
        description = "Only set true when the user explicitly asked to reopen a completed task"
    )]
    pub allow_reopen_completed: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SubtaskInput {
    #[schemars(description = "Existing subtask id; omit when adding a new subtask")]
    pub id: Option<String>,
    pub title: String,
    pub completed: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum AcceptanceCriterionInput {
    Text(String),
    Detailed(AcceptanceCriterionFields),
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AcceptanceCriterionFields {
    #[schemars(description = "Existing criterion id; omit when adding a new criterion")]
    pub id: Option<String>,
    pub title: String,
    pub completed: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatusInput {
    Todo,
    InProgress,
    Blocked,
    Done,
}

impl From<TaskStatusInput> for TaskStatus {
    fn from(value: TaskStatusInput) -> Self {
        match value {
            TaskStatusInput::Todo => TaskStatus::Todo,
            TaskStatusInput::InProgress => TaskStatus::InProgress,
            TaskStatusInput::Blocked => TaskStatus::Blocked,
            TaskStatusInput::Done => TaskStatus::Done,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PriorityInput {
    Low,
    Medium,
    High,
}

impl From<PriorityInput> for Priority {
    fn from(value: PriorityInput) -> Self {
        match value {
            PriorityInput::Low => Priority::Low,
            PriorityInput::Medium => Priority::Medium,
            PriorityInput::High => Priority::High,
        }
    }
}

fn make_subtasks(items: Vec<SubtaskInput>) -> Vec<Subtask> {
    items
        .into_iter()
        .map(|item| Subtask {
            id: item.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
            title: item.title.trim().to_string(),
            completed: item.completed.unwrap_or(false),
        })
        .collect()
}

fn make_acceptance_criteria(items: Vec<AcceptanceCriterionInput>) -> Vec<AcceptanceCriterion> {
    items
        .into_iter()
        .map(|item| match item {
            AcceptanceCriterionInput::Text(title) => AcceptanceCriterion {
                id: Uuid::new_v4().to_string(),
                title: title.trim().to_string(),
                completed: false,
            },
            AcceptanceCriterionInput::Detailed(item) => AcceptanceCriterion {
                id: item.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
                title: item.title.trim().to_string(),
                completed: item.completed.unwrap_or(false),
            },
        })
        .filter(|item| !item.title.is_empty())
        .collect()
}

fn now_label() -> String {
    Local::now().format("%Y-%m-%d %H:%M").to_string()
}

fn normalize_request_id(request_id: Option<String>) -> Result<Option<String>, String> {
    let Some(request_id) = request_id else {
        return Ok(None);
    };
    let request_id = request_id.trim();
    if request_id.is_empty() {
        return Err("request_id cannot be empty when provided".to_string());
    }
    if request_id.chars().count() > MAX_REQUEST_ID_LENGTH {
        return Err(format!(
            "request_id cannot exceed {MAX_REQUEST_ID_LENGTH} characters"
        ));
    }
    Ok(Some(request_id.to_string()))
}

fn request_fingerprint(value: &impl Serialize) -> String {
    let payload = serde_json::to_vec(value).expect("serialize request fingerprint");
    format!("{:x}", Sha256::digest(payload))
}

#[tool_router]
impl TodoMcpServer {
    #[tool(
        description = "List TodoList projects. Call this before creating a task when the target project id is unknown.",
        annotations(
            title = "List TodoList projects",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn list_projects(&self) -> CallToolResult {
        match self.store.load_workspace() {
            Ok(Some(workspace)) => Self::success(json!({
                "workspaceVersion": workspace.version,
                "projects": workspace.projects,
            })),
            Ok(None) => Self::error(
                "TodoList workspace is not initialized; open the desktop app once before using MCP",
            ),
            Err(error) => Self::error(format!("Could not read TodoList projects: {error}")),
        }
    }

    #[tool(
        description = "Create a TodoList project. Pass a stable request_id so a transport retry returns the original project instead of creating a duplicate.",
        annotations(
            title = "Create TodoList project",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn create_project(
        &self,
        Parameters(mut input): Parameters<CreateProjectInput>,
    ) -> CallToolResult {
        let name = input.name.trim();
        if name.is_empty() {
            return Self::error("Project name cannot be empty");
        }
        input.name = name.to_string();
        input.request_id = match normalize_request_id(input.request_id) {
            Ok(request_id) => request_id,
            Err(error) => return Self::error(error),
        };
        let color = input
            .color
            .as_deref()
            .map(str::trim)
            .filter(|color| !color.is_empty())
            .unwrap_or("#1264f4")
            .to_string();
        input.color = Some(color.clone());

        let project = Project {
            id: Uuid::new_v4().to_string(),
            name: input.name.clone(),
            color,
        };
        let entity_id = project.id.clone();
        let created_project = project.clone();
        let mutate = move |mut workspace: task_core::Workspace| {
            if workspace
                .projects
                .iter()
                .any(|project| project.name.eq_ignore_ascii_case(&created_project.name))
            {
                return Err(format!(
                    "A project named '{}' already exists",
                    created_project.name
                ));
            }
            workspace.version += 1;
            workspace.projects.push(created_project);
            Ok(workspace)
        };

        let mutation = if let Some(request_id) = input.request_id.as_deref() {
            self.store.mutate_workspace_idempotent(
                "create_project",
                request_id,
                &request_fingerprint(&input),
                &entity_id,
                mutate,
            )
        } else {
            self.store
                .mutate_workspace(mutate)
                .map(|workspace| IdempotentMutationResult {
                    workspace,
                    entity_id: entity_id.clone(),
                    replayed: false,
                })
        };

        match mutation {
            Ok(result) => {
                let Some(project) = result
                    .workspace
                    .projects
                    .iter()
                    .find(|project| project.id == result.entity_id)
                else {
                    return Self::error(
                        "The original project for this request no longer exists; use a new request_id",
                    );
                };
                Self::success(json!({
                    "workspaceVersion": result.workspace.version,
                    "replayed": result.replayed,
                    "project": project,
                }))
            }
            Err(error) => Self::error(format!("Could not create TodoList project: {error}")),
        }
    }

    #[tool(
        description = "List current TodoList tasks with optional filters and cursor pagination. Returns task versions and separate attachments/images metadata. File content is not returned; users add files in desktop task details.",
        annotations(
            title = "List TodoList tasks",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn list_tasks(&self, Parameters(input): Parameters<ListTasksInput>) -> CallToolResult {
        let limit = input.limit.unwrap_or(DEFAULT_TASK_PAGE_SIZE);
        if !(1..=MAX_TASK_PAGE_SIZE).contains(&limit) {
            return Self::error(format!("limit must be between 1 and {MAX_TASK_PAGE_SIZE}"));
        }
        match self.store.load_workspace() {
            Ok(Some(workspace)) => {
                let requested_status = input.status.map(TaskStatus::from);
                let include_done = input.include_done.unwrap_or(true);
                let include_archived = input.include_archived.unwrap_or(false);
                let tasks: Vec<_> = workspace
                    .tasks
                    .into_iter()
                    .filter(|task| {
                        input
                            .project_id
                            .as_ref()
                            .map(|project_id| &task.project_id == project_id)
                            .unwrap_or(true)
                    })
                    .filter(|task| {
                        requested_status
                            .as_ref()
                            .map(|status| &task.status == status)
                            .unwrap_or(true)
                    })
                    .filter(|task| include_done || task.status != TaskStatus::Done)
                    .filter(|task| include_archived || !task.archived)
                    .collect();
                let total_count = tasks.len();
                let start = match input.cursor.as_deref() {
                    Some(cursor) => match tasks.iter().position(|task| task.id == cursor) {
                        Some(index) => index + 1,
                        None => {
                            return Self::error(
                                "cursor is invalid for the current list_tasks filters; restart without a cursor",
                            )
                        }
                    },
                    None => 0,
                };
                let end = (start + limit as usize).min(total_count);
                let next_cursor = if end < total_count {
                    Some(tasks[end - 1].id.clone())
                } else {
                    None
                };
                let page = tasks
                    .into_iter()
                    .skip(start)
                    .take(limit as usize)
                    .collect::<Vec<_>>();
                Self::success(json!({
                    "workspaceVersion": workspace.version,
                    "count": page.len(),
                    "totalCount": total_count,
                    "limit": limit,
                    "nextCursor": next_cursor,
                    "tasks": page,
                }))
            }
            Ok(None) => Self::error(
                "TodoList workspace is not initialized; open the desktop app once before using MCP",
            ),
            Err(error) => Self::error(format!("Could not read TodoList tasks: {error}")),
        }
    }

    #[tool(
        description = "Read one TodoList task by id before updating it, including separate attachments/images metadata (originalName, mediaType, size, storageKey, addedAt). File content is not returned. Add, remove, and preview files through desktop task details; MCP has no file upload tool.",
        annotations(
            title = "Get TodoList task",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    fn get_task(&self, Parameters(input): Parameters<GetTaskInput>) -> CallToolResult {
        match self.store.load_workspace() {
            Ok(Some(workspace)) => match workspace
                .tasks
                .into_iter()
                .find(|task| task.id == input.task_id)
            {
                Some(task) => Self::success(json!({
                    "workspaceVersion": workspace.version,
                    "task": task,
                })),
                None => Self::error(format!("Task '{}' was not found", input.task_id)),
            },
            Ok(None) => Self::error(
                "TodoList workspace is not initialized; open the desktop app once before using MCP",
            ),
            Err(error) => Self::error(format!("Could not read TodoList task: {error}")),
        }
    }

    #[tool(
        description = "Create an unpinned TodoList task in an existing project. Pass a stable request_id so a transport retry returns the original task instead of creating a duplicate. This never opens the desktop note.",
        annotations(
            title = "Create TodoList task",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn create_task(&self, Parameters(mut input): Parameters<CreateTaskInput>) -> CallToolResult {
        let title = input.title.trim();
        if title.is_empty() {
            return Self::error("Task title cannot be empty");
        }
        input.title = title.to_string();
        input.request_id = match normalize_request_id(input.request_id) {
            Ok(request_id) => request_id,
            Err(error) => return Self::error(error),
        };
        let task = Task {
            id: Uuid::new_v4().to_string(),
            project_id: input.project_id.clone(),
            title: input.title.clone(),
            description: input.description.clone().unwrap_or_default(),
            status: TaskStatus::Todo,
            priority: input.priority.unwrap_or(PriorityInput::Medium).into(),
            due_label: input
                .due_date
                .clone()
                .unwrap_or_else(|| "未安排".to_string()),
            due_date: input
                .due_date
                .clone()
                .unwrap_or_else(|| "9999-12-31".to_string()),
            tags: input.tags.clone().unwrap_or_default(),
            source: "Codex 创建".to_string(),
            archived: false,
            pinned: false,
            version: 1,
            subtasks: make_subtasks(input.subtasks.clone().unwrap_or_default()),
            acceptance_criteria: make_acceptance_criteria(
                input.acceptance_criteria.clone().unwrap_or_default(),
            ),
            attachments: vec![],
            images: vec![],
            dependencies: input.dependencies.clone().unwrap_or_default(),
            activity: vec![ActivityItem {
                id: Uuid::new_v4().to_string(),
                action: "Codex 创建任务".to_string(),
                actor: "codex".to_string(),
                at: now_label(),
            }],
        };
        let project_id = task.project_id.clone();
        let entity_id = task.id.clone();
        let created_task = task.clone();
        let mutate = move |mut workspace: task_core::Workspace| {
            if !workspace
                .projects
                .iter()
                .any(|project| project.id == project_id)
            {
                return Err(format!("Project '{project_id}' was not found"));
            }
            workspace.version += 1;
            workspace.tasks.push(created_task);
            Ok(workspace)
        };
        let mutation = if let Some(request_id) = input.request_id.as_deref() {
            self.store.mutate_workspace_idempotent(
                "create_task",
                request_id,
                &request_fingerprint(&input),
                &entity_id,
                mutate,
            )
        } else {
            self.store
                .mutate_workspace(mutate)
                .map(|workspace| IdempotentMutationResult {
                    workspace,
                    entity_id: entity_id.clone(),
                    replayed: false,
                })
        };

        match mutation {
            Ok(result) => {
                let Some(task) = result
                    .workspace
                    .tasks
                    .iter()
                    .find(|task| task.id == result.entity_id)
                else {
                    return Self::error(
                        "The original task for this request no longer exists; use a new request_id",
                    );
                };
                Self::success(json!({
                    "workspaceVersion": result.workspace.version,
                    "replayed": result.replayed,
                    "task": task,
                }))
            }
            Err(error) => Self::error(format!("Could not create TodoList task: {error}")),
        }
    }

    #[tool(
        description = "Update fields on an existing TodoList task using optimistic version checking. Read the task first and pass its latest version. Existing attachments and images are preserved automatically; this tool cannot add, remove, or replace files.",
        annotations(
            title = "Update TodoList task",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    fn update_task(&self, Parameters(input): Parameters<UpdateTaskInput>) -> CallToolResult {
        let task_id = input.task_id.clone();
        let expected_version = input.expected_version;
        let update_result = self.store.mutate_workspace(move |mut workspace| {
            let task = workspace
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .ok_or_else(|| format!("Task '{task_id}' was not found"))?;

            if task.version != expected_version {
                return Err(serde_json::to_string(&json!({
                    "code": "version_conflict",
                    "message": "The task changed after it was read. Read it again and preserve newer user edits.",
                    "expectedVersion": expected_version,
                    "currentVersion": task.version,
                    "currentTask": task,
                }))
                .expect("serialize version conflict"));
            }

            if matches!(task.status, TaskStatus::Done)
                && input
                    .status
                    .map(TaskStatus::from)
                    .is_some_and(|status| status != TaskStatus::Done)
                && !input.allow_reopen_completed.unwrap_or(false)
            {
                return Err("A completed task cannot be reopened unless the user explicitly requested it and allow_reopen_completed is true".to_string());
            }

            let completing_task = !matches!(task.status, TaskStatus::Done)
                && input
                    .status
                    .map(TaskStatus::from)
                    .is_some_and(|status| status == TaskStatus::Done);
            let replacing_acceptance_criteria = input.acceptance_criteria.is_some();

            let mut changed = false;
            if let Some(title) = input.title {
                let title = title.trim().to_string();
                if title.is_empty() {
                    return Err("Task title cannot be empty".to_string());
                }
                task.title = title;
                changed = true;
            }
            if let Some(description) = input.description {
                task.description = description;
                changed = true;
            }
            if let Some(status) = input.status {
                task.status = status.into();
                changed = true;
            }
            if let Some(priority) = input.priority {
                task.priority = priority.into();
                changed = true;
            }
            if let Some(due_date) = input.due_date {
                task.due_label = due_date.clone();
                task.due_date = due_date;
                changed = true;
            }
            if let Some(tags) = input.tags {
                task.tags = tags;
                changed = true;
            }
            if let Some(subtasks) = input.subtasks {
                task.subtasks = make_subtasks(subtasks);
                changed = true;
            }
            if let Some(criteria) = input.acceptance_criteria {
                task.acceptance_criteria = make_acceptance_criteria(criteria);
                changed = true;
            }
            if let Some(dependencies) = input.dependencies {
                task.dependencies = dependencies;
                changed = true;
            }
            if (completing_task || replacing_acceptance_criteria)
                && matches!(task.status, TaskStatus::Done)
                && task
                    .acceptance_criteria
                    .iter()
                    .any(|criterion| !criterion.completed)
            {
                return Err(
                    "A task cannot be completed until all acceptance criteria are confirmed"
                        .to_string(),
                );
            }
            if !changed {
                return Err("No task fields were provided to update".to_string());
            }

            task.version += 1;
            task.push_activity(ActivityItem {
                id: Uuid::new_v4().to_string(),
                action: "Codex 更新任务".to_string(),
                actor: "codex".to_string(),
                at: now_label(),
            });
            workspace.version += 1;
            Ok(workspace)
        });

        match update_result {
            Ok(workspace) => {
                let task = workspace
                    .tasks
                    .iter()
                    .find(|task| task.id == input.task_id)
                    .expect("updated task remains in workspace");
                Self::success(json!({
                    "workspaceVersion": workspace.version,
                    "task": task,
                }))
            }
            Err(error) => Self::error(format!("Could not update TodoList task: {error}")),
        }
    }
}

#[tool_handler(
    name = "todolist",
    version = "0.2.0",
    instructions = "TodoList is a local-first task app. Task details support separate attachments and images, added and managed through the desktop UI. get_task and list_tasks return file metadata, not file content; update_task preserves these fields. This MCP server has no file upload, removal, or preview tools. Never claim that a path written in a description attaches a file. Read current data before writing and follow list_tasks nextCursor when the full result matters. For create_project and create_task, pass a stable unique request_id and reuse it only to retry identical input. For update_task, pass the task's latest version as expected_version. If a version conflict occurs, read the task again: preserve newer user edits, merge only non-conflicting fields, and skip same-field conflicts unless the user explicitly asked to replace them. Never reopen a completed task unless the user explicitly requested it and allow_reopen_completed is true. MCP-created tasks are never pinned and this server never opens the desktop note."
)]
impl ServerHandler for TodoMcpServer {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};
    use task_core::{ManagedFile, Project, Workspace};

    fn result_json(result: CallToolResult) -> serde_json::Value {
        assert_eq!(result.is_error, Some(false));
        let text = result.content[0].as_text().expect("tool result is text");
        serde_json::from_str(&text.text).expect("tool result contains JSON")
    }

    fn test_server() -> (TodoMcpServer, PathBuf) {
        let database_path = std::env::temp_dir().join(format!(
            "todolist-mcp-{}-{}.sqlite",
            std::process::id(),
            Uuid::new_v4()
        ));
        let store = SqliteTaskStore::open(&database_path).expect("open test store");
        store
            .save_workspace(&Workspace {
                version: 1,
                projects: vec![Project {
                    id: "project-1".to_string(),
                    name: "TodoList".to_string(),
                    color: "#1264f4".to_string(),
                }],
                tasks: vec![],
            })
            .expect("seed test workspace");
        (TodoMcpServer::new(store), database_path)
    }

    fn remove_database(database_path: &PathBuf) {
        let _ = fs::remove_file(database_path.with_extension("sqlite-shm"));
        let _ = fs::remove_file(database_path.with_extension("sqlite-wal"));
        let _ = fs::remove_file(database_path);
    }

    fn backup_test_task(server: &TodoMcpServer) -> Workspace {
        let result = server.create_task(Parameters(
            serde_json::from_value(json!({
                "project_id": "project-1", "title": "Backup task"
            }))
            .unwrap(),
        ));
        assert_eq!(result.is_error, Some(false));
        server.store.load_workspace().unwrap().unwrap()
    }

    fn update_title(server: &TodoMcpServer, task: &Task, title: &str) -> CallToolResult {
        server.update_task(Parameters(
            serde_json::from_value(json!({
                "task_id": task.id, "expected_version": task.version, "title": title
            }))
            .unwrap(),
        ))
    }

    #[test]
    fn restored_tasks_reject_old_mcp_versions_after_repeated_restore_and_deletion() {
        let (server, database_path) = test_server();
        let backup = backup_test_task(&server);
        let original = &backup.tasks[0];
        assert_eq!(update_title(&server, original, "v2").is_error, Some(false));
        let mut held = vec![
            original.clone(),
            server.store.load_workspace().unwrap().unwrap().tasks[0].clone(),
        ];
        for _ in 0..3 {
            let mut candidate = backup.clone();
            candidate.version = server.store.load_workspace_version().unwrap().unwrap() + 1;
            let restored = server.store.restore_workspace(&candidate).unwrap();
            assert!(
                restored.tasks[0].version > held.iter().map(|task| task.version).max().unwrap()
            );
            for stale in &held {
                let result = update_title(&server, stale, "stale overwrite");
                assert_eq!(result.is_error, Some(true));
                assert!(result.content[0]
                    .as_text()
                    .unwrap()
                    .text
                    .contains("version_conflict"));
            }
            assert_eq!(
                server.store.load_workspace().unwrap().unwrap().tasks[0].title,
                original.title
            );
            assert_eq!(
                update_title(&server, &restored.tasks[0], "fresh write").is_error,
                Some(false)
            );
            held.push(server.store.load_workspace().unwrap().unwrap().tasks[0].clone());
            server
                .store
                .mutate_workspace(|mut current| {
                    current.version += 1;
                    current.tasks.clear();
                    Ok(current)
                })
                .unwrap();
        }
        drop(server);
        remove_database(&database_path);
    }

    #[test]
    fn restore_and_concurrent_mcp_write_are_serialized() {
        let (server, database_path) = test_server();
        let backup = backup_test_task(&server);
        let task = backup.tasks[0].clone();
        let mut candidate = backup.clone();
        candidate.version += 1;
        let competing_store = SqliteTaskStore::open(&database_path).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let other_barrier = barrier.clone();
        let write = std::thread::spawn(move || {
            let competitor = TodoMcpServer::new(competing_store);
            other_barrier.wait();
            update_title(&competitor, &task, "concurrent MCP")
        });
        barrier.wait();
        let restore = server.store.restore_workspace(&candidate);
        let write = write.join().unwrap();
        if restore.is_err() {
            assert!(restore.unwrap_err().contains("workspace version conflict"));
            assert_eq!(write.is_error, Some(false));
            candidate.version = server.store.load_workspace_version().unwrap().unwrap() + 1;
            server.store.restore_workspace(&candidate).unwrap();
        } else {
            assert_eq!(write.is_error, Some(true));
        }
        assert_eq!(
            update_title(&server, &backup.tasks[0], "late MCP").is_error,
            Some(true)
        );
        assert_eq!(
            server.store.load_workspace().unwrap().unwrap().tasks[0].title,
            "Backup task"
        );
        drop(server);
        remove_database(&database_path);
    }

    #[test]
    fn desktop_saves_cannot_bypass_acceptance_but_legacy_tasks_remain_editable() {
        let (server, database_path) = test_server();
        let backup = backup_test_task(&server);
        let mut pending = backup.clone();
        pending.version += 1;
        pending.tasks[0].acceptance_criteria = vec![AcceptanceCriterion {
            id: "a".into(),
            title: "Verify".into(),
            completed: false,
        }];
        server.store.save_workspace(&pending).unwrap();
        let mut invalid = pending.clone();
        invalid.version += 1;
        invalid.tasks[0].status = TaskStatus::Done;
        assert!(server
            .store
            .save_workspace(&invalid)
            .unwrap_err()
            .contains("验收标准"));
        assert!(server
            .store
            .mutate_workspace(|_| Ok(invalid.clone()))
            .unwrap_err()
            .contains("验收标准"));
        assert!(server
            .store
            .mutate_workspace_idempotent("test", "test", "test", "test", |_| Ok(invalid.clone()))
            .unwrap_err()
            .contains("验收标准"));
        assert_eq!(
            server.store.load_workspace_version().unwrap(),
            Some(pending.version)
        );

        // A restored legacy done task may contain old unconfirmed criteria.
        let legacy = server.store.restore_workspace(&invalid).unwrap();
        assert_eq!(
            update_title(&server, &legacy.tasks[0], "legacy title edit").is_error,
            Some(false)
        );
        let mut edited = server.store.load_workspace().unwrap().unwrap();
        edited.version += 1;
        edited.tasks[0].pinned = true;
        server.store.save_workspace(&edited).unwrap();
        edited.version += 1;
        edited.tasks[0]
            .acceptance_criteria
            .push(AcceptanceCriterion {
                id: "new".into(),
                title: "New requirement".into(),
                completed: false,
            });
        assert!(server.store.save_workspace(&edited).is_err());
        edited.tasks[0].status = TaskStatus::Todo;
        server.store.save_workspace(&edited).unwrap();
        edited.version += 1;
        for criterion in &mut edited.tasks[0].acceptance_criteria {
            criterion.completed = true;
        }
        edited.tasks[0].status = TaskStatus::Done;
        server.store.save_workspace(&edited).unwrap();
        drop(server);
        remove_database(&database_path);
    }

    #[test]
    fn creates_unpinned_codex_task() {
        let (server, database_path) = test_server();
        let result = server.create_task(Parameters(CreateTaskInput {
            request_id: None,
            project_id: "project-1".to_string(),
            title: "Write MCP tests".to_string(),
            description: None,
            priority: Some(PriorityInput::High),
            due_date: None,
            tags: None,
            subtasks: None,
            acceptance_criteria: None,
            dependencies: None,
        }));

        assert_eq!(result.is_error, Some(false));
        let workspace = server.store.load_workspace().unwrap().unwrap();
        assert_eq!(workspace.tasks.len(), 1);
        assert!(!workspace.tasks[0].pinned);
        assert_eq!(workspace.tasks[0].source, "Codex 创建");
        assert!(workspace.tasks[0].attachments.is_empty());
        assert!(workspace.tasks[0].images.is_empty());
        drop(server);
        remove_database(&database_path);
    }

    #[test]
    fn mcp_updates_preserve_desktop_managed_file_fields() {
        let (server, database_path) = test_server();
        backup_test_task(&server);
        let workspace = server
            .store
            .mutate_workspace(|mut current| {
                current.version += 1;
                current.tasks[0].version += 1;
                current.tasks[0].attachments.push(ManagedFile {
                    id: "file-1".into(),
                    original_name: "report.pdf".into(),
                    media_type: "application/pdf".into(),
                    size: 12,
                    storage_key: "attachments/file-1.pdf".into(),
                    added_at: "now".into(),
                });
                Ok(current)
            })
            .unwrap();
        assert_eq!(
            update_title(&server, &workspace.tasks[0], "Renamed").is_error,
            Some(false)
        );
        let updated = server.store.load_workspace().unwrap().unwrap();
        assert_eq!(
            updated.tasks[0].attachments[0].storage_key,
            "attachments/file-1.pdf"
        );
        drop(server);
        remove_database(&database_path);
    }

    #[test]
    fn rejects_stale_updates_and_unapproved_reopen() {
        let (server, database_path) = test_server();
        server.create_task(Parameters(CreateTaskInput {
            request_id: None,
            project_id: "project-1".to_string(),
            title: "Task".to_string(),
            description: None,
            priority: None,
            due_date: None,
            tags: None,
            subtasks: None,
            acceptance_criteria: None,
            dependencies: None,
        }));
        let task_id = server.store.load_workspace().unwrap().unwrap().tasks[0]
            .id
            .clone();

        let complete = server.update_task(Parameters(UpdateTaskInput {
            task_id: task_id.clone(),
            expected_version: 1,
            title: None,
            description: None,
            status: Some(TaskStatusInput::Done),
            priority: None,
            due_date: None,
            tags: None,
            subtasks: None,
            acceptance_criteria: None,
            dependencies: None,
            allow_reopen_completed: None,
        }));
        assert_eq!(complete.is_error, Some(false));

        let stale = server.update_task(Parameters(UpdateTaskInput {
            task_id: task_id.clone(),
            expected_version: 1,
            title: Some("Stale title".to_string()),
            description: None,
            status: None,
            priority: None,
            due_date: None,
            tags: None,
            subtasks: None,
            acceptance_criteria: None,
            dependencies: None,
            allow_reopen_completed: None,
        }));
        assert_eq!(stale.is_error, Some(true));

        let reopen = server.update_task(Parameters(UpdateTaskInput {
            task_id,
            expected_version: 2,
            title: None,
            description: None,
            status: Some(TaskStatusInput::Todo),
            priority: None,
            due_date: None,
            tags: None,
            subtasks: None,
            acceptance_criteria: None,
            dependencies: None,
            allow_reopen_completed: None,
        }));
        assert_eq!(reopen.is_error, Some(true));

        drop(server);
        remove_database(&database_path);
    }

    #[test]
    fn requires_acceptance_confirmation_before_completion() {
        let (server, database_path) = test_server();
        server.create_task(Parameters(CreateTaskInput {
            request_id: None,
            project_id: "project-1".to_string(),
            title: "Task with acceptance".to_string(),
            description: None,
            priority: None,
            due_date: None,
            tags: None,
            subtasks: None,
            acceptance_criteria: Some(vec![AcceptanceCriterionInput::Text("Verified".to_string())]),
            dependencies: None,
        }));
        let task = server.store.load_workspace().unwrap().unwrap().tasks[0].clone();

        let rejected = server.update_task(Parameters(UpdateTaskInput {
            task_id: task.id.clone(),
            expected_version: task.version,
            title: None,
            description: None,
            status: Some(TaskStatusInput::Done),
            priority: None,
            due_date: None,
            tags: None,
            subtasks: None,
            acceptance_criteria: None,
            dependencies: None,
            allow_reopen_completed: None,
        }));
        assert_eq!(rejected.is_error, Some(true));

        let completed = server.update_task(Parameters(UpdateTaskInput {
            task_id: task.id,
            expected_version: task.version,
            title: None,
            description: None,
            status: Some(TaskStatusInput::Done),
            priority: None,
            due_date: None,
            tags: None,
            subtasks: None,
            acceptance_criteria: Some(vec![AcceptanceCriterionInput::Detailed(
                AcceptanceCriterionFields {
                    id: Some(task.acceptance_criteria[0].id.clone()),
                    title: task.acceptance_criteria[0].title.clone(),
                    completed: Some(true),
                },
            )]),
            dependencies: None,
            allow_reopen_completed: None,
        }));
        assert_eq!(completed.is_error, Some(false));

        drop(server);
        remove_database(&database_path);
    }

    #[test]
    fn creates_project_once_when_a_request_is_retried() {
        let (server, database_path) = test_server();
        let input = || CreateProjectInput {
            request_id: Some("create-project-planning".to_string()),
            name: "Planning".to_string(),
            color: Some("#7c3aed".to_string()),
        };

        let first = result_json(server.create_project(Parameters(input())));
        drop(server);

        let reopened = TodoMcpServer::new(SqliteTaskStore::open(&database_path).unwrap());
        let replay = result_json(reopened.create_project(Parameters(input())));
        let workspace = reopened.store.load_workspace().unwrap().unwrap();

        assert_eq!(workspace.projects.len(), 2);
        assert_eq!(first["project"]["id"], replay["project"]["id"]);
        assert_eq!(first["replayed"], false);
        assert_eq!(replay["replayed"], true);

        let conflict = reopened.create_project(Parameters(CreateProjectInput {
            request_id: Some("create-project-planning".to_string()),
            name: "Different project".to_string(),
            color: Some("#7c3aed".to_string()),
        }));
        assert_eq!(conflict.is_error, Some(true));

        drop(reopened);
        remove_database(&database_path);
    }

    #[test]
    fn creates_task_once_when_a_request_is_retried() {
        let (server, database_path) = test_server();
        let input = || CreateTaskInput {
            request_id: Some("create-task-release-check".to_string()),
            project_id: "project-1".to_string(),
            title: "Check release".to_string(),
            description: Some("Run the release checks".to_string()),
            priority: Some(PriorityInput::High),
            due_date: None,
            tags: None,
            subtasks: None,
            acceptance_criteria: None,
            dependencies: None,
        };

        let first = result_json(server.create_task(Parameters(input())));
        let replay = result_json(server.create_task(Parameters(input())));
        let workspace = server.store.load_workspace().unwrap().unwrap();

        assert_eq!(workspace.tasks.len(), 1);
        assert_eq!(workspace.version, 2);
        assert_eq!(first["task"]["id"], replay["task"]["id"]);
        assert_eq!(first["replayed"], false);
        assert_eq!(replay["replayed"], true);

        drop(server);
        remove_database(&database_path);
    }

    #[test]
    fn paginates_filtered_tasks_with_a_returned_cursor() {
        let (server, database_path) = test_server();
        for index in 1..=5 {
            server.create_task(Parameters(CreateTaskInput {
                request_id: None,
                project_id: "project-1".to_string(),
                title: format!("Task {index}"),
                description: None,
                priority: None,
                due_date: None,
                tags: None,
                subtasks: None,
                acceptance_criteria: None,
                dependencies: None,
            }));
        }

        let first = result_json(server.list_tasks(Parameters(ListTasksInput {
            project_id: Some("project-1".to_string()),
            status: None,
            include_done: None,
            include_archived: None,
            limit: Some(2),
            cursor: None,
        })));
        let second = result_json(server.list_tasks(Parameters(ListTasksInput {
            project_id: Some("project-1".to_string()),
            status: None,
            include_done: None,
            include_archived: None,
            limit: Some(2),
            cursor: first["nextCursor"].as_str().map(str::to_string),
        })));
        let third = result_json(server.list_tasks(Parameters(ListTasksInput {
            project_id: Some("project-1".to_string()),
            status: None,
            include_done: None,
            include_archived: None,
            limit: Some(2),
            cursor: second["nextCursor"].as_str().map(str::to_string),
        })));

        assert_eq!(first["count"], 2);
        assert_eq!(first["totalCount"], 5);
        assert_eq!(second["count"], 2);
        assert_eq!(third["count"], 1);
        assert!(third["nextCursor"].is_null());
        assert_eq!(first["tasks"][0]["title"], "Task 1");
        assert_eq!(second["tasks"][0]["title"], "Task 3");
        assert_eq!(third["tasks"][0]["title"], "Task 5");

        let invalid_limit = server.list_tasks(Parameters(ListTasksInput {
            project_id: None,
            status: None,
            include_done: None,
            include_archived: None,
            limit: Some(101),
            cursor: None,
        }));
        assert_eq!(invalid_limit.is_error, Some(true));

        let invalid_cursor = server.list_tasks(Parameters(ListTasksInput {
            project_id: Some("project-1".to_string()),
            status: None,
            include_done: None,
            include_archived: None,
            limit: Some(2),
            cursor: Some("not-a-returned-cursor".to_string()),
        }));
        assert_eq!(invalid_cursor.is_error, Some(true));

        drop(server);
        remove_database(&database_path);
    }
}
