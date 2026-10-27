use crate::*;
use query::{Archive, Filter, MatchMode, Projection, Sort};
use std::collections::HashSet;

#[derive(Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ListProjectsInput {
    pub query: Option<String>,
    pub match_mode: Option<MatchMode>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct StatsInput {
    #[serde(default)]
    pub filter: Filter,
    pub today: Option<String>,
}
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct InfoInput {
    pub since: Option<String>,
    pub limit: Option<usize>,
}
#[derive(Serialize, Deserialize)]
struct PageCursor {
    version: u64,
    fingerprint: String,
    index: usize,
}
pub(crate) fn page_start(
    cursor: Option<&str>,
    version: u64,
    fingerprint: &str,
    total: usize,
) -> Result<usize, String> {
    let Some(cursor) = cursor else { return Ok(0) };
    if cursor.len() > 2048 {
        return Err("invalid cursor".into());
    }
    let cursor: PageCursor = serde_json::from_str(cursor).map_err(|_| "invalid cursor")?;
    if cursor.version != version || cursor.fingerprint != fingerprint || cursor.index > total {
        return Err("cursor is stale or belongs to different filters/sort/projection; restart without cursor".into());
    }
    Ok(cursor.index)
}
pub(crate) fn page_cursor(
    version: u64,
    fingerprint: String,
    index: usize,
    total: usize,
) -> Option<String> {
    (index < total).then(|| {
        serde_json::to_string(&PageCursor {
            version,
            fingerprint,
            index,
        })
        .unwrap()
    })
}
impl TodoMcpServer {
    pub(crate) fn wrap(result: Result<serde_json::Value, String>) -> CallToolResult {
        match result {
            Ok(v) => Self::success(v),
            Err(e) => Self::error(e),
        }
    }
    pub(crate) fn workspace(&self, op: &'static str) -> Result<Workspace, String> {
        self.read_workspace(op)?
            .ok_or_else(|| "TodoList workspace is not initialized; open desktop app once".into())
    }
    pub(crate) fn projected(&self, task: &Task, projection: Projection) -> serde_json::Value {
        match projection {
            Projection::Full => self.task_output(task),
            Projection::Compact => query::compact(task),
            Projection::Ids => json!({"id":task.id,"version":task.version}),
        }
    }
    pub(crate) fn write_task_output(
        &self,
        task: &Task,
        response: Option<Projection>,
    ) -> serde_json::Value {
        self.projected(task, response.unwrap_or(Projection::Ids))
    }
    pub(crate) fn projects_v2(
        &self,
        input: ListProjectsInput,
    ) -> Result<serde_json::Value, String> {
        let w = self.workspace("list_projects")?;
        let limit = input.limit.unwrap_or(15);
        if !(1..=100).contains(&limit) {
            return Err("limit must be 1 through 100".into());
        }
        let mode = input.match_mode.unwrap_or(MatchMode::Contains);
        if mode == MatchMode::Fuzzy {
            return Err("project names support exact or contains only".into());
        }
        let q = input.query.as_deref().map(query::normalized);
        if q.as_ref()
            .is_some_and(|q| q.is_empty() || q.chars().count() > 128)
        {
            return Err("project query must be 1 through 128 characters".into());
        }
        let matches: Vec<_> = w
            .projects
            .iter()
            .filter(|p| {
                q.as_ref().is_none_or(|q| {
                    if mode == MatchMode::Exact {
                        query::normalized(&p.name) == *q
                    } else {
                        query::normalized(&p.name).contains(q)
                    }
                })
            })
            .collect();
        let fingerprint = request_fingerprint(&json!({"query":q,"mode":mode}));
        let start = page_start(
            input.cursor.as_deref(),
            w.version,
            &fingerprint,
            matches.len(),
        )?;
        let end = (start + limit).min(matches.len());
        Ok(
            json!({"workspaceVersion":w.version,"projects":matches[start..end],"count":end-start,"totalCount":matches.len(),"ambiguous":q.is_some()&&matches.len()>1,"nextCursor":page_cursor(w.version,fingerprint,end,matches.len())}),
        )
    }
    pub(crate) fn tasks_v2(&self, input: ListTasksInput) -> Result<serde_json::Value, String> {
        let w = self.workspace("list_tasks")?;
        let limit = input.limit.unwrap_or(DEFAULT_TASK_PAGE_SIZE);
        if !(1..=MAX_TASK_PAGE_SIZE).contains(&limit) {
            return Err("limit must be 1 through 100".into());
        }
        let mut filter = input.filter.unwrap_or_default();
        if let Some(project) = input.project_id {
            if filter.project_id.as_ref().is_some_and(|p| p != &project) {
                return Err("conflicting project filters".into());
            }
            filter.project_id = Some(project);
        }
        if let Some(status) = input.status {
            let status = serde_json::to_value(TaskStatus::from(status))
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();
            if filter
                .statuses
                .as_ref()
                .is_some_and(|s| !s.contains(&status))
            {
                return Err("conflicting status filters".into());
            }
            filter.statuses = Some(vec![status]);
        }
        if input.include_archived == Some(true) && filter.archived == Archive::Active {
            filter.archived = Archive::All;
        }
        let sort = input.sort.unwrap_or(if filter.query.is_some() {
            Sort::Relevance
        } else {
            Sort::Manual
        });
        let projection = input.projection.unwrap_or_default();
        let fingerprint = request_fingerprint(
            &json!({"filter":filter,"sort":sort,"projection":projection,"includeDone":input.include_done.unwrap_or(true)}),
        );
        let matches: Vec<_> = query::select(&w, &filter, sort)?
            .into_iter()
            .filter(|h| input.include_done.unwrap_or(true) || h.task.status != TaskStatus::Done)
            .collect();
        let start = page_start(
            input.cursor.as_deref(),
            w.version,
            &fingerprint,
            matches.len(),
        )?;
        let end = (start + limit as usize).min(matches.len());
        let tasks: Vec<_> = matches[start..end]
            .iter()
            .map(|h| {
                let mut result = self.projected(h.task, projection);
                if !h.reasons.is_empty() {
                    result["matches"] = json!(h.reasons);
                }
                result
            })
            .collect();
        Ok(
            json!({"workspaceVersion":w.version,"projection":projection,"count":tasks.len(),"totalCount":matches.len(),"limit":limit,"nextCursor":page_cursor(w.version,fingerprint,end,matches.len()),"tasks":tasks}),
        )
    }
    pub(crate) fn get_v2(&self, input: GetTaskInput) -> Result<serde_json::Value, String> {
        let w = self.workspace("get_task")?;
        if !input.task_id.is_empty() && input.task_ids.is_some() {
            return Err("use task_id or task_ids, not both".into());
        }
        let many = input.task_ids.is_some();
        let ids = input
            .task_ids
            .clone()
            .unwrap_or_else(|| vec![input.task_id.clone()]);
        if ids.is_empty()
            || ids.len() > 20
            || ids.iter().any(|id| id.is_empty())
            || ids.iter().collect::<HashSet<_>>().len() != ids.len()
        {
            return Err("provide 1 through 20 unique explicit task ids".into());
        }
        let mut requested = HashSet::<String>::new();
        if let Some(sections) = &input.sections {
            for section in sections {
                match section.as_str() {
                    "core" => {
                        requested.extend(
                            [
                                "id",
                                "title",
                                "projectId",
                                "status",
                                "priority",
                                "version",
                                "dueDate",
                                "completedAt",
                                "archived",
                            ]
                            .map(str::to_string),
                        );
                    }
                    "files" => {
                        requested.extend(["attachments", "images"].map(str::to_string));
                    }
                    "checklists" => {
                        requested.extend(["subtasks", "acceptanceCriteria"].map(str::to_string));
                    }
                    "description" | "subtasks" | "acceptanceCriteria" | "dependencies" | "tags"
                    | "attachments" | "images" => {
                        requested.insert(section.clone());
                    }
                    _ => return Err("unsupported section; activity uses get_task_activity".into()),
                }
            }
        }
        if let Some(fields) = &input.fields {
            for field in fields {
                if ![
                    "id",
                    "projectId",
                    "title",
                    "description",
                    "status",
                    "priority",
                    "dueDate",
                    "dueLabel",
                    "completedAt",
                    "tags",
                    "source",
                    "archived",
                    "pinned",
                    "version",
                    "subtasks",
                    "acceptanceCriteria",
                    "dependencies",
                    "attachments",
                    "images",
                ]
                .contains(&field.as_str())
                {
                    return Err(
                        "unsupported task field; activity/content is not available here".into(),
                    );
                }
                requested.insert(field.clone());
            }
        }
        let constrained = input.sections.is_some() || input.fields.is_some();
        let mut tasks = Vec::new();
        let mut missing = Vec::new();
        for id in ids {
            let Some(task) = w.tasks.iter().find(|t| t.id == id) else {
                missing.push(id);
                continue;
            };
            let projection = input.projection.unwrap_or(if many {
                Projection::Compact
            } else {
                Projection::Full
            });
            let mut value = if constrained {
                self.task_output(task)
            } else {
                self.projected(task, projection)
            };
            if constrained {
                value
                    .as_object_mut()
                    .unwrap()
                    .retain(|key, _| key == "id" || key == "version" || requested.contains(key));
            }
            if value.get("description").is_some()
                && (constrained
                    || input.description_start.is_some()
                    || input.description_limit.is_some())
            {
                let start = input.description_start.unwrap_or(0);
                let limit = input.description_limit.unwrap_or(4000);
                if !(1..=16000).contains(&limit) {
                    return Err("description_limit must be 1 through 16000 characters".into());
                }
                let chars: Vec<_> = task.description.chars().collect();
                if start > chars.len() {
                    return Err("description_start exceeds description length".into());
                }
                let end = (start + limit).min(chars.len());
                value["description"] = json!(chars[start..end].iter().collect::<String>());
                value["descriptionRange"] = json!({"start":start,"end":end,"total":chars.len(),"nextStart":if end<chars.len(){Some(end)}else{None},"unit":"unicode characters"});
            }
            tasks.push(value);
        }
        if !many {
            let task = tasks.into_iter().next().ok_or("Task was not found")?;
            Ok(json!({"workspaceVersion":w.version,"task":task}))
        } else {
            Ok(json!({"workspaceVersion":w.version,"tasks":tasks,"missingIds":missing}))
        }
    }
    pub(crate) fn update_v2(
        &self,
        mut input: UpdateTaskInput,
    ) -> Result<serde_json::Value, String> {
        input.request_id = normalize_request_id(input.request_id)?;
        if input.edits.needs_receipt() && input.request_id.is_none() {
            return Err(
                "request_id is required for local edits to prevent duplicate append/add retries"
                    .into(),
            );
        }
        let mutate = |mut w: Workspace| {
            let changes = match mutations::apply(&mut w, &input, self.client) {
                Ok(v) => v,
                Err(error) => {
                    if input.response == Some(Projection::Full) {
                        if let Ok(mut conflict) = serde_json::from_str::<serde_json::Value>(&error)
                        {
                            if conflict["code"] == "version_conflict" {
                                if let Some(task) = w.tasks.iter().find(|t| t.id == input.task_id) {
                                    conflict["currentTask"] = self.task_output(task);
                                }
                                return Err(conflict.to_string());
                            }
                        }
                    }
                    return Err(error);
                }
            };
            w.version = w
                .version
                .checked_add(1)
                .filter(|v| *v <= MAX_SAFE_INTEGER)
                .ok_or("workspace version limit reached")?;
            let task = w.tasks.iter().find(|t| t.id == input.task_id).unwrap();
            // Retry storage is only a mutation receipt, never a task snapshot or
            // runtime file projection. Expanded data is rendered after commit.
            let response = if input.request_id.is_some() {
                Some(Projection::Ids)
            } else {
                input.response
            };
            let result = json!({"task":self.write_task_output(task,response),"changedItems":changes["changedItems"]});
            Ok((w, result))
        };
        if let Some(key) = &input.request_id {
            let result = self.store.mutate_with_receipt(
                &self.client.operation("update_task_v2"),
                key,
                &request_fingerprint(&input),
                mutate,
            )?;
            let mut receipt = result.receipt;
            if input.response.is_some_and(|p| p != Projection::Ids) {
                // On replay this transaction read the current persisted task.
                // Files are projected now, so removed associations cannot return
                // an old available path, and missing copies are checked again.
                receipt["appliedTask"] =
                    json!({"id":input.task_id,"version":receipt["task"]["version"]});
                receipt["appliedWorkspaceVersion"] = receipt["workspaceVersion"].clone();
                receipt["workspaceVersion"] = json!(result.workspace.version);
                receipt["task"] = result
                    .workspace
                    .tasks
                    .iter()
                    .find(|t| t.id == input.task_id)
                    .map(|t| self.write_task_output(t, input.response))
                    .unwrap_or(serde_json::Value::Null);
            }
            Ok(receipt)
        } else {
            let mut result = None;
            let saved = self.store.mutate_workspace(|w| {
                let (w, value) = mutate(w)?;
                result = Some(value);
                Ok(w)
            })?;
            let mut value = result.unwrap();
            value["workspaceVersion"] = json!(saved.version);
            Ok(value)
        }
    }
    pub(crate) fn batch_v2(
        &self,
        mut input: mutations::BatchInput,
    ) -> Result<serde_json::Value, String> {
        input.request_id = normalize_required_request_id(input.request_id)?;
        self.store
            .mutate_with_receipt(
                &self.client.operation("batch_update"),
                &input.request_id,
                &request_fingerprint(&input),
                |mut w| {
                    let result = mutations::batch(&mut w, &input, self.client)?;
                    Ok((w, result))
                },
            )
            .map(|result| result.receipt)
    }
    pub(crate) fn info_v2(&self, input: InfoInput) -> Result<serde_json::Value, String> {
        let mut result = self
            .store
            .changes_since(input.since.as_deref(), input.limit.unwrap_or(100))?;
        result["mcpVersion"] = json!(env!("CARGO_PKG_VERSION"));
        result["channel"] = json!(if cfg!(feature = "production") {
            "production"
        } else {
            "development"
        });
        result["buildIdentity"] = json!(format!(
            "todolist/{}/{}",
            env!("CARGO_PKG_VERSION"),
            result["channel"].as_str().unwrap()
        ));
        result["capabilities"] = json!([
            "compact_queries",
            "field_projections",
            "filtered_statistics",
            "atomic_batch",
            "idempotent_local_edits",
            "managed_file_read",
            "scoped_content_search",
            "id_only_changes"
        ]);
        result["runtimeHealth"] = json!("server process only; no client connectivity claim");
        result["contentFormats"] = json!([
            "utf8_text",
            "pdf_pages",
            "docx_paragraphs",
            "pptx_slides",
            "xlsx_sheet_range",
            "png",
            "jpeg",
            "gif",
            "webp",
            "bmp"
        ]);
        result["limits"] = json!({"defaultTaskPage":15,"maximumTaskPage":100,"batchTasks":50,"batchProjects":20,"textAttachmentBytes":65536,"fileInputBytes":file_content::MAX_INPUT_BYTES,"defaultContentBytes":16384,"maximumTextPreviewBytes":65536,"maximumOriginalImageBytes":262144,"imagePixels":16000000,"imageEdge":8192,"thumbnailEdge":1024,"changeRevisions":128,"changeJournalBytes":2097152,"ocr":"not provided; requires_ocr","binaryOffice":"unsupported"});
        Ok(result)
    }
}
