use serde::{Deserialize, Serialize};

pub const MAX_TASK_ACTIVITY_ITEMS: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    InProgress,
    Blocked,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subtask {
    pub id: String,
    pub title: String,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceCriterion {
    pub id: String,
    pub title: String,
    pub completed: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredAcceptanceCriterion {
    Text(String),
    Detailed(AcceptanceCriterion),
}

fn deserialize_acceptance_criteria<'de, D>(
    deserializer: D,
) -> Result<Vec<AcceptanceCriterion>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let stored = Vec::<StoredAcceptanceCriterion>::deserialize(deserializer)?;
    Ok(stored
        .into_iter()
        .enumerate()
        .map(|(index, criterion)| match criterion {
            StoredAcceptanceCriterion::Text(title) => AcceptanceCriterion {
                id: format!("legacy-criterion-{index}"),
                title,
                completed: false,
            },
            StoredAcceptanceCriterion::Detailed(criterion) => criterion,
        })
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityItem {
    pub id: String,
    pub action: String,
    pub actor: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedFile {
    pub id: String,
    pub original_name: String,
    pub media_type: String,
    pub size: u64,
    pub storage_key: String,
    pub added_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub due_label: String,
    pub due_date: String,
    pub tags: Vec<String>,
    pub source: String,
    #[serde(default)]
    pub archived: bool,
    pub pinned: bool,
    pub version: u64,
    pub subtasks: Vec<Subtask>,
    #[serde(default, deserialize_with = "deserialize_acceptance_criteria")]
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    #[serde(default)]
    pub attachments: Vec<ManagedFile>,
    #[serde(default)]
    pub images: Vec<ManagedFile>,
    pub dependencies: Vec<String>,
    pub activity: Vec<ActivityItem>,
}

impl Task {
    pub fn push_activity(&mut self, item: ActivityItem) {
        self.activity.push(item);
        self.trim_activity();
    }

    pub fn trim_activity(&mut self) {
        if self.activity.len() > MAX_TASK_ACTIVITY_ITEMS {
            self.activity
                .drain(..self.activity.len() - MAX_TASK_ACTIVITY_ITEMS);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub version: u64,
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
}

impl Workspace {
    pub fn validate_completion_changes(&self, previous: &Workspace) -> Result<(), String> {
        for task in &self.tasks {
            if task.status != TaskStatus::Done {
                continue;
            }
            let old = previous.tasks.iter().find(|old| old.id == task.id);
            for criterion in task
                .acceptance_criteria
                .iter()
                .filter(|item| !item.completed)
            {
                // Keep legacy done tasks usable, but never add an unconfirmed
                // requirement or newly complete a task with pending criteria.
                let unchanged_legacy = old.is_some_and(|old| {
                    old.status == TaskStatus::Done
                        && old.acceptance_criteria.iter().any(|item| {
                            item.id == criterion.id
                                && item.title == criterion.title
                                && !item.completed
                        })
                });
                if !unchanged_legacy {
                    return Err(format!(
                        "任务“{}”尚有未确认的验收标准，请逐项确认后再完成",
                        task.title
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn trim_activity(&mut self) {
        for task in &mut self.tasks {
            task.trim_activity();
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for task in &self.tasks {
            if !self
                .projects
                .iter()
                .any(|project| project.id == task.project_id)
            {
                return Err(format!("task {} references an unknown project", task.id));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_tasks_that_reference_missing_projects() {
        let workspace = Workspace {
            version: 1,
            projects: vec![],
            tasks: vec![Task {
                id: "task-1".into(),
                project_id: "missing".into(),
                title: "Task".into(),
                description: String::new(),
                status: TaskStatus::Todo,
                priority: Priority::Medium,
                due_label: "Today".into(),
                due_date: "2026-09-01".into(),
                tags: vec![],
                source: "user".into(),
                archived: false,
                pinned: false,
                version: 1,
                subtasks: vec![],
                acceptance_criteria: vec![],
                attachments: vec![],
                images: vec![],
                dependencies: vec![],
                activity: vec![],
            }],
        };

        assert!(workspace.validate().is_err());
    }

    #[test]
    fn reads_legacy_acceptance_text_and_missing_archive_flag() {
        let workspace: Workspace = serde_json::from_str(
            r##"{
              "version": 1,
              "projects": [{"id":"project-1","name":"TodoList","color":"#1264f4"}],
              "tasks": [{
                "id":"task-1","projectId":"project-1","title":"Task","description":"",
                "status":"todo","priority":"medium","dueLabel":"Today","dueDate":"2026-09-01",
                "tags":[],"source":"user","pinned":false,"version":1,"subtasks":[],
                "acceptanceCriteria":["Legacy criterion"],"dependencies":[],"activity":[]
              }]
            }"##,
        )
        .expect("deserialize legacy workspace");

        assert!(!workspace.tasks[0].archived);
        assert_eq!(
            workspace.tasks[0].acceptance_criteria[0].title,
            "Legacy criterion"
        );
        assert!(!workspace.tasks[0].acceptance_criteria[0].completed);
        assert!(workspace.tasks[0].attachments.is_empty());
        assert!(workspace.tasks[0].images.is_empty());
    }

    #[test]
    fn task_activity_keeps_only_the_newest_items() {
        let mut task = Task {
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
            attachments: vec![],
            images: vec![],
            dependencies: vec![],
            activity: vec![],
        };
        for index in 0..(MAX_TASK_ACTIVITY_ITEMS + 5) {
            task.push_activity(ActivityItem {
                id: index.to_string(),
                action: "edit".into(),
                actor: "user".into(),
                at: "now".into(),
            });
        }
        assert_eq!(task.activity.len(), MAX_TASK_ACTIVITY_ITEMS);
        assert_eq!(task.activity[0].id, "5");
        assert_eq!(task.activity.last().unwrap().id, "104");
    }
}
