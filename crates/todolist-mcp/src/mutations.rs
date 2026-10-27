use crate::*;
use rmcp::schemars;
use std::collections::HashSet;

#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LocalEdits {
    pub description_edit: Option<DescriptionEdit>,
    pub subtask_edits: Option<Vec<ItemEdit>>,
    pub criterion_edits: Option<Vec<ItemEdit>>,
    pub dependency_edits: Option<Vec<DependencyEdit>>,
    pub tags_add: Option<Vec<String>>,
    pub tags_remove: Option<Vec<String>>,
    pub project_id: Option<String>,
    pub archived: Option<bool>,
}
impl LocalEdits {
    pub fn needs_receipt(&self) -> bool {
        self.description_edit.is_some()
            || self.subtask_edits.is_some()
            || self.criterion_edits.is_some()
            || self.dependency_edits.is_some()
            || self.tags_add.is_some()
            || self.tags_remove.is_some()
    }
}
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DescriptionAction {
    Append,
    Prepend,
    Replace,
    Delete,
}
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescriptionEdit {
    pub action: DescriptionAction,
    pub text: Option<String>,
    pub old: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemAction {
    Add,
    Rename,
    SetCompleted,
    Remove,
}
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemEdit {
    pub action: ItemAction,
    pub item_id: Option<String>,
    pub title: Option<String>,
    pub completed: Option<bool>,
}
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DependencyAction {
    Add,
    Replace,
    Remove,
}
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DependencyEdit {
    pub action: DependencyAction,
    pub old: Option<String>,
    pub text: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ProjectEdit {
    pub project_id: String,
    pub name: Option<String>,
    pub color: Option<String>,
}
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct BatchInput {
    pub request_id: String,
    pub expected_workspace_version: Option<u64>,
    #[serde(default)]
    pub tasks: Vec<UpdateTaskInput>,
    #[serde(default)]
    pub projects: Vec<ProjectEdit>,
}
fn required_text(value: Option<&str>) -> Result<String, String> {
    let v = value.unwrap_or("").trim();
    if v.is_empty() {
        Err("nonempty text is required".into())
    } else {
        Ok(v.into())
    }
}
fn unique_occurrence(text: &str, old: &str) -> Result<(), String> {
    if old.is_empty() {
        return Err("old text must not be empty".into());
    }
    // Count overlapping matches too: replacing 'aa' in 'aaa' is ambiguous.
    if text
        .char_indices()
        .filter(|(i, _)| text[*i..].starts_with(old))
        .take(2)
        .count()
        != 1
    {
        return Err("old text must match exactly once (zero or multiple matches rejected)".into());
    }
    Ok(())
}
fn edit_description(current: &str, edit: &DescriptionEdit) -> Result<String, String> {
    let text = edit.text.as_deref().unwrap_or("");
    if text.len() > 65536 {
        return Err("description edit text exceeds 64 KiB".into());
    }
    match edit.action {
        DescriptionAction::Append => Ok(format!("{current}{text}")),
        DescriptionAction::Prepend => Ok(format!("{text}{current}")),
        DescriptionAction::Replace | DescriptionAction::Delete => {
            let old = edit.old.as_deref().ok_or("old text is required")?;
            unique_occurrence(current, old)?;
            Ok(current.replacen(
                old,
                if matches!(edit.action, DescriptionAction::Delete) {
                    ""
                } else {
                    text
                },
                1,
            ))
        }
    }
}
fn item_edits(
    items: &mut Vec<Subtask>,
    edits: &[ItemEdit],
) -> Result<Vec<serde_json::Value>, String> {
    if edits.len() > 50 {
        return Err("at most 50 checklist edits".into());
    }
    let mut result = Vec::new();
    for edit in edits {
        let id = if matches!(edit.action, ItemAction::Add) {
            edit.item_id
                .clone()
                .unwrap_or_else(|| Uuid::new_v4().to_string())
        } else {
            edit.item_id.clone().ok_or("item_id is required")?
        };
        if matches!(edit.action, ItemAction::Add) {
            if items.iter().any(|s| s.id == id) {
                return Err("checklist item id already exists".into());
            }
            items.push(Subtask {
                id: id.clone(),
                title: required_text(edit.title.as_deref())?,
                completed: edit.completed.unwrap_or(false),
            });
        } else {
            let index = items
                .iter()
                .position(|s| s.id == id)
                .ok_or("checklist item id not found; re-read latest items")?;
            match edit.action {
                ItemAction::Rename => items[index].title = required_text(edit.title.as_deref())?,
                ItemAction::SetCompleted => {
                    items[index].completed = edit.completed.ok_or("completed is required")?
                }
                ItemAction::Remove => {
                    items.remove(index);
                }
                ItemAction::Add => unreachable!(),
            }
        }
        result.push(json!({"itemId":id,"action":edit.action}));
    }
    Ok(result)
}
pub fn apply(
    workspace: &mut Workspace,
    input: &UpdateTaskInput,
    client: AiClient,
) -> Result<serde_json::Value, String> {
    if input.expected_version > MAX_SAFE_INTEGER {
        return Err("expected_version exceeds exact integer range".into());
    }
    if let Some(project) = &input.edits.project_id {
        if !workspace.projects.iter().any(|p| p.id == *project) {
            return Err("destination project_id not found".into());
        }
    }
    let task = workspace
        .tasks
        .iter_mut()
        .find(|t| t.id == input.task_id)
        .ok_or("task_id not found")?;
    if task.version != input.expected_version {
        return Err(serde_json::to_string(&json!({"code":"version_conflict","taskId":task.id,"expectedVersion":input.expected_version,"currentVersion":task.version,"message":"re-read required fields and preserve newer user changes"})).unwrap());
    }
    let old = task.clone();
    let mut changed = false;
    let mut items = json!({});
    if let Some(status) = input.status {
        let status: TaskStatus = status.into();
        if old.status == TaskStatus::Done
            && status != TaskStatus::Done
            && !input.allow_reopen_completed.unwrap_or(false)
        {
            return Err(
                "completed task requires explicit user reopen and allow_reopen_completed=true"
                    .into(),
            );
        }
        task.status = status;
        changed = true;
    }
    if let Some(title) = &input.title {
        task.title = required_text(Some(title))?;
        changed = true;
    }
    if input.description.is_some() && input.edits.description_edit.is_some() {
        return Err("description and description_edit are mutually exclusive".into());
    }
    if let Some(description) = &input.description {
        task.description = description.clone();
        changed = true;
    }
    if let Some(edit) = &input.edits.description_edit {
        task.description = edit_description(&task.description, edit)?;
        changed = true;
    }
    if let Some(priority) = input.priority {
        task.priority = priority.into();
        changed = true;
    }
    if let Some(due) = &input.due_date {
        task.due_date = normalize_due_date(Some(due))?;
        task.due_label = due_label(&task.due_date);
        changed = true;
    }
    if input.tags.is_some() && (input.edits.tags_add.is_some() || input.edits.tags_remove.is_some())
    {
        return Err("tags replacement and tag edits are mutually exclusive".into());
    }
    if let Some(tags) = &input.tags {
        task.tags = tags.clone();
        changed = true;
    }
    if let Some(tags) = &input.edits.tags_remove {
        task.tags.retain(|t| !tags.contains(t));
        changed = true;
    }
    if let Some(tags) = &input.edits.tags_add {
        for t in tags {
            let t = required_text(Some(t))?;
            if !task.tags.contains(&t) {
                task.tags.push(t);
            }
        }
        changed = true;
    }
    if input.subtasks.is_some() && input.edits.subtask_edits.is_some() {
        return Err("subtasks replacement and edits are mutually exclusive".into());
    }
    if input.acceptance_criteria.is_some() && input.edits.criterion_edits.is_some() {
        return Err("criteria replacement and edits are mutually exclusive".into());
    }
    if let Some(subtasks) = &input.subtasks {
        task.subtasks = make_subtasks(subtasks.clone());
        changed = true;
    }
    if let Some(criteria) = &input.acceptance_criteria {
        task.acceptance_criteria = make_acceptance_criteria(criteria.clone());
        changed = true;
    }
    if let Some(edits) = &input.edits.subtask_edits {
        items["subtasks"] = json!(item_edits(&mut task.subtasks, edits)?);
        changed = true;
    }
    if let Some(edits) = &input.edits.criterion_edits {
        let mut converted = task
            .acceptance_criteria
            .iter()
            .map(|c| Subtask {
                id: c.id.clone(),
                title: c.title.clone(),
                completed: c.completed,
            })
            .collect();
        items["acceptanceCriteria"] = json!(item_edits(&mut converted, edits)?);
        task.acceptance_criteria = converted
            .into_iter()
            .map(|c| AcceptanceCriterion {
                id: c.id,
                title: c.title,
                completed: c.completed,
            })
            .collect();
        changed = true;
    }
    if input.dependencies.is_some() && input.edits.dependency_edits.is_some() {
        return Err("dependencies replacement and edits are mutually exclusive".into());
    }
    if let Some(dependencies) = &input.dependencies {
        task.dependencies = dependencies.clone();
        changed = true;
    }
    if let Some(edits) = &input.edits.dependency_edits {
        if edits.len() > 50 {
            return Err("at most 50 dependency edits".into());
        }
        for edit in edits {
            match edit.action {
                DependencyAction::Add => {
                    task.dependencies.push(required_text(edit.text.as_deref())?)
                }
                DependencyAction::Remove | DependencyAction::Replace => {
                    let old = edit.old.as_deref().ok_or("dependency old text required")?;
                    let indexes: Vec<_> = task
                        .dependencies
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| t.as_str() == old)
                        .map(|(i, _)| i)
                        .collect();
                    if indexes.len() != 1 {
                        return Err("dependency old text must identify exactly one item".into());
                    }
                    if matches!(edit.action, DependencyAction::Remove) {
                        task.dependencies.remove(indexes[0]);
                    } else {
                        task.dependencies[indexes[0]] = required_text(edit.text.as_deref())?;
                    }
                }
            }
        }
        changed = true;
    }
    if let Some(project) = &input.edits.project_id {
        task.project_id = project.clone();
        changed = true;
    }
    if let Some(archived) = input.edits.archived {
        task.archived = archived;
        if archived {
            task.pinned = false;
        }
        changed = true;
    }
    if !changed {
        return Err("No task fields were provided to update".into());
    }
    if task.status == TaskStatus::Done
        && (old.status != TaskStatus::Done || input.acceptance_criteria.is_some())
        && task.acceptance_criteria.iter().any(|c| !c.completed)
    {
        return Err(
            "A task cannot be completed until all acceptance criteria are confirmed".into(),
        );
    }
    task.completed_at = if task.status != TaskStatus::Done {
        None
    } else if old.status == TaskStatus::Done {
        old.completed_at
    } else {
        Some(chrono::Utc::now().to_rfc3339())
    };
    task.version = task
        .version
        .checked_add(1)
        .filter(|v| *v <= MAX_SAFE_INTEGER)
        .ok_or("task version limit reached")?;
    task.push_activity(ActivityItem {
        id: Uuid::new_v4().to_string(),
        action: format!("{} 更新任务", client.label()),
        actor: client.actor().into(),
        at: chrono::Utc::now().to_rfc3339(),
    });
    Ok(json!({"id":task.id,"version":task.version,"changedItems":items}))
}

pub fn batch(
    workspace: &mut Workspace,
    input: &BatchInput,
    client: AiClient,
) -> Result<serde_json::Value, String> {
    if input.tasks.len() > 50
        || input.projects.len() > 20
        || (input.tasks.is_empty() && input.projects.is_empty())
    {
        return Err("batch requires 1..50 tasks and/or 1..20 projects".into());
    }
    if (!input.projects.is_empty() && input.expected_workspace_version.is_none())
        || input
            .expected_workspace_version
            .is_some_and(|v| v != workspace.version)
    {
        return Err("workspace_version_conflict: re-read workspace version".into());
    }
    let mut ids = HashSet::new();
    let mut tasks = Vec::new();
    for edit in &input.tasks {
        if !ids.insert(edit.task_id.clone()) {
            return Err("duplicate task id in batch".into());
        }
        tasks.push(apply(workspace, edit, client)?);
    }
    ids.clear();
    let mut projects = Vec::new();
    for edit in &input.projects {
        if !ids.insert(edit.project_id.clone()) {
            return Err("duplicate project id in batch".into());
        }
        if let Some(name) = &edit.name {
            let name = required_text(Some(name))?;
            if workspace.projects.iter().any(|p| {
                p.id != edit.project_id && query::normalized(&p.name) == query::normalized(&name)
            }) {
                return Err("project name is ambiguous or already exists".into());
            }
        }
        let project = workspace
            .projects
            .iter_mut()
            .find(|p| p.id == edit.project_id)
            .ok_or("project_id not found")?;
        if edit.name.is_none() && edit.color.is_none() {
            return Err("project patch is empty".into());
        }
        if let Some(name) = &edit.name {
            project.name = required_text(Some(name))?;
        }
        if let Some(color) = &edit.color {
            let c = color.trim();
            if !c.starts_with('#')
                || ![4, 7].contains(&c.len())
                || !c[1..].chars().all(|c| c.is_ascii_hexdigit())
            {
                return Err("project color must be #RGB or #RRGGBB".into());
            }
            project.color = c.to_ascii_lowercase();
        }
        projects.push(json!({"id":project.id,"updated":true}));
    }
    workspace.version = workspace
        .version
        .checked_add(1)
        .filter(|v| *v <= MAX_SAFE_INTEGER)
        .ok_or("workspace version limit reached")?;
    Ok(json!({"atomic":true,"tasks":tasks,"projects":projects}))
}
