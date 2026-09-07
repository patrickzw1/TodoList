use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const MAX_TASK_ACTIVITY_ITEMS: usize = 100;
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

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
    /// Reorder one complete project/archive group while leaving every other
    /// task in its existing slot. Task contents and task versions are untouched.
    pub fn reorder_tasks(
        &mut self,
        project_id: &str,
        archived: bool,
        ordered_task_ids: &[String],
    ) -> Result<bool, String> {
        if !self.projects.iter().any(|project| project.id == project_id) {
            return Err(format!("Project '{project_id}' was not found"));
        }
        let ordered_set: HashSet<_> = ordered_task_ids.iter().map(String::as_str).collect();
        if ordered_set.len() != ordered_task_ids.len() {
            return Err("task_ids contains duplicate task ids".to_string());
        }
        for task_id in ordered_task_ids {
            let task = self
                .tasks
                .iter()
                .find(|task| task.id == *task_id)
                .ok_or_else(|| format!("Task '{task_id}' was not found"))?;
            if task.project_id != project_id {
                return Err(format!("Task '{task_id}' belongs to a different project"));
            }
            if task.archived != archived {
                return Err(format!("Task '{task_id}' has a different archived state"));
            }
        }

        let current_order: Vec<_> = self
            .tasks
            .iter()
            .filter(|task| task.project_id == project_id && task.archived == archived)
            .map(|task| task.id.clone())
            .collect();
        if current_order.len() != ordered_task_ids.len()
            || current_order
                .iter()
                .any(|task_id| !ordered_set.contains(task_id.as_str()))
        {
            return Err("task_ids must contain every task in the requested project and archived state exactly once".to_string());
        }
        if current_order == ordered_task_ids {
            return Ok(false);
        }

        let next_workspace_version = self
            .version
            .checked_add(1)
            .filter(|version| *version <= MAX_SAFE_INTEGER)
            .ok_or_else(|| "Workspace version limit reached; cannot reorder tasks".to_string())?;

        let tasks_by_id: HashMap<_, _> = self
            .tasks
            .iter()
            .filter(|task| task.project_id == project_id && task.archived == archived)
            .map(|task| (task.id.clone(), task.clone()))
            .collect();
        let mut ordered = ordered_task_ids.iter();
        for task in &mut self.tasks {
            if task.project_id == project_id && task.archived == archived {
                let task_id = ordered.next().expect("validated order length");
                *task = tasks_by_id.get(task_id).expect("validated task id").clone();
            }
        }
        self.version = next_workspace_version;
        Ok(true)
    }

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

    fn task(id: &str, project_id: &str, archived: bool) -> Task {
        Task {
            id: id.into(),
            project_id: project_id.into(),
            title: id.into(),
            description: String::new(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            due_label: "Today".into(),
            due_date: "2026-09-07".into(),
            tags: vec![],
            source: "user".into(),
            archived,
            pinned: false,
            version: 1,
            subtasks: vec![],
            acceptance_criteria: vec![],
            attachments: vec![],
            images: vec![],
            dependencies: vec![],
            activity: vec![],
        }
    }

    #[test]
    fn reorders_only_the_requested_group_without_changing_task_versions() {
        let mut workspace = Workspace {
            version: 7,
            projects: vec![
                Project {
                    id: "p1".into(),
                    name: "One".into(),
                    color: "#111111".into(),
                },
                Project {
                    id: "p2".into(),
                    name: "Two".into(),
                    color: "#222222".into(),
                },
            ],
            tasks: vec![
                task("a", "p1", false),
                task("x", "p2", false),
                task("b", "p1", false),
                task("z", "p1", true),
                task("c", "p1", false),
            ],
        };

        assert!(workspace
            .reorder_tasks("p1", false, &["c".into(), "a".into(), "b".into()])
            .unwrap());
        assert_eq!(workspace.version, 8);
        assert_eq!(
            workspace
                .tasks
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c", "x", "a", "z", "b"]
        );
        assert!(workspace.tasks.iter().all(|task| task.version == 1));
        assert!(!workspace
            .reorder_tasks("p1", false, &["c".into(), "a".into(), "b".into()])
            .unwrap());
        assert_eq!(workspace.version, 8);
    }

    #[test]
    fn rejects_incomplete_duplicate_and_cross_group_orders_without_partial_changes() {
        let workspace = Workspace {
            version: 3,
            projects: vec![
                Project {
                    id: "p1".into(),
                    name: "One".into(),
                    color: "#111111".into(),
                },
                Project {
                    id: "p2".into(),
                    name: "Two".into(),
                    color: "#222222".into(),
                },
            ],
            tasks: vec![
                task("a", "p1", false),
                task("b", "p1", false),
                task("x", "p2", false),
                task("z", "p1", true),
            ],
        };
        for order in [
            vec!["a".into()],
            vec!["a".into(), "a".into()],
            vec!["a".into(), "x".into()],
            vec!["a".into(), "z".into()],
        ] {
            let mut candidate = workspace.clone();
            assert!(candidate.reorder_tasks("p1", false, &order).is_err());
            assert_eq!(candidate.version, workspace.version);
            assert_eq!(
                candidate
                    .tasks
                    .iter()
                    .map(|task| &task.id)
                    .collect::<Vec<_>>(),
                workspace
                    .tasks
                    .iter()
                    .map(|task| &task.id)
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn rejects_reorder_when_workspace_version_cannot_increment_safely() {
        let mut workspace = Workspace {
            version: MAX_SAFE_INTEGER,
            projects: vec![Project {
                id: "p1".into(),
                name: "One".into(),
                color: "#111111".into(),
            }],
            tasks: vec![task("a", "p1", false), task("b", "p1", false)],
        };
        assert!(workspace
            .reorder_tasks("p1", false, &["b".into(), "a".into()])
            .is_err());
        assert_eq!(
            workspace
                .tasks
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }
}
