use chrono::{DateTime, FixedOffset, Local, NaiveDate};
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use task_core::{Task, Workspace};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    #[default]
    Compact,
    Full,
    Ids,
}
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    Exact,
    #[default]
    Contains,
    Fuzzy,
}
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Archive {
    #[default]
    Active,
    Archived,
    All,
}
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Manual,
    Relevance,
    DueDate,
    CompletedAt,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Filter {
    pub query: Option<String>,
    /// title, description, subtasks, acceptance_criteria, dependencies, tags, files.
    pub fields: Option<Vec<String>>,
    pub match_mode: MatchMode,
    pub project_id: Option<String>,
    pub statuses: Option<Vec<String>>,
    pub priorities: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub archived: Archive,
    pub due_from: Option<String>,
    pub due_to: Option<String>,
    pub completed_from: Option<String>,
    pub completed_to: Option<String>,
    pub has_attachments: Option<bool>,
    pub has_images: Option<bool>,
    /// Completion calendar dates use this fixed UTC offset; omit for server local offset.
    pub utc_offset_minutes: Option<i32>,
}

pub fn normalized(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| {
            let c = if ('\u{ff01}'..='\u{ff5e}').contains(&c) {
                char::from_u32(c as u32 - 0xfee0).unwrap()
            } else {
                c
            };
            c.to_lowercase()
        })
        .collect()
}
pub fn calendar(value: &str) -> Result<NaiveDate, String> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| "expected real YYYY-MM-DD calendar date")?;
    if date.format("%Y-%m-%d").to_string() != value || value.starts_with("0000") {
        return Err("expected real YYYY-MM-DD calendar date".into());
    }
    Ok(date)
}
pub fn offset(minutes: Option<i32>) -> Result<FixedOffset, String> {
    match minutes {
        Some(m) if (-840..=840).contains(&m) => Ok(FixedOffset::east_opt(m * 60).unwrap()),
        Some(_) => Err("utc_offset_minutes must be -840 through 840".into()),
        None => Ok(*Local::now().offset()),
    }
}
impl Filter {
    pub fn validate(&self) -> Result<(), String> {
        offset(self.utc_offset_minutes)?;
        for (start, end) in [
            (&self.due_from, &self.due_to),
            (&self.completed_from, &self.completed_to),
        ] {
            if let Some(value) = start {
                calendar(value)?;
            }
            if let Some(value) = end {
                calendar(value)?;
            }
            if start.as_ref().zip(end.as_ref()).is_some_and(|(a, b)| a > b) {
                return Err("date range start exceeds end".into());
            }
        }
        if self
            .query
            .as_ref()
            .is_some_and(|q| normalized(q).is_empty() || q.chars().count() > 128)
        {
            return Err("query must contain 1 through 128 nonblank characters".into());
        }
        if self.match_mode == MatchMode::Fuzzy
            && self
                .query
                .as_ref()
                .is_some_and(|q| normalized(q).chars().count() > 64)
        {
            return Err("fuzzy queries are limited to 64 characters".into());
        }
        if let Some(fields) = &self.fields {
            if fields.is_empty()
                || fields.len() > 7
                || fields.iter().any(|f| {
                    ![
                        "title",
                        "description",
                        "subtasks",
                        "acceptance_criteria",
                        "dependencies",
                        "tags",
                        "files",
                    ]
                    .contains(&f.as_str())
                })
            {
                return Err("invalid search fields".into());
            }
        }
        if self.statuses.as_ref().is_some_and(|v| {
            v.is_empty()
                || v.iter()
                    .any(|s| !["todo", "in_progress", "blocked", "done"].contains(&s.as_str()))
        }) {
            return Err("invalid statuses".into());
        }
        if self.priorities.as_ref().is_some_and(|v| {
            v.is_empty()
                || v.iter()
                    .any(|s| !["low", "medium", "high"].contains(&s.as_str()))
        }) {
            return Err("invalid priorities".into());
        }
        if self
            .tags
            .as_ref()
            .is_some_and(|v| v.len() > 50 || v.iter().any(|s| s.len() > 512))
        {
            return Err("tag filter budget exceeded".into());
        }
        Ok(())
    }
}
fn in_range(value: Option<&str>, from: &Option<String>, to: &Option<String>) -> bool {
    if from.is_none() && to.is_none() {
        return true;
    }
    value.is_some_and(|date| {
        from.as_ref().is_none_or(|v| date >= v.as_str())
            && to.as_ref().is_none_or(|v| date <= v.as_str())
    })
}
pub fn completion_day(task: &Task, zone: FixedOffset) -> Option<String> {
    task.completed_at
        .as_deref()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&zone).format("%Y-%m-%d").to_string())
}
struct SearchField<'a> {
    field: &'static str,
    value: &'a str,
    item_id: Option<&'a str>,
    completed: Option<bool>,
    file_id: Option<&'a str>,
}
impl<'a> SearchField<'a> {
    fn text(field: &'static str, value: &'a str) -> Self {
        Self {
            field,
            value,
            item_id: None,
            completed: None,
            file_id: None,
        }
    }
}
fn field_values(task: &Task) -> Vec<SearchField<'_>> {
    let mut values = vec![
        SearchField::text("title", &task.title),
        SearchField::text("description", &task.description),
    ];
    values.extend(task.subtasks.iter().map(|s| SearchField {
        item_id: Some(&s.id),
        completed: Some(s.completed),
        ..SearchField::text("subtasks", &s.title)
    }));
    values.extend(task.acceptance_criteria.iter().map(|s| SearchField {
        item_id: Some(&s.id),
        completed: Some(s.completed),
        ..SearchField::text("acceptance_criteria", &s.title)
    }));
    values.extend(
        task.dependencies
            .iter()
            .map(|s| SearchField::text("dependencies", s)),
    );
    values.extend(task.tags.iter().map(|s| SearchField::text("tags", s)));
    values.extend(
        task.attachments
            .iter()
            .chain(&task.images)
            .map(|s| SearchField {
                file_id: Some(&s.id),
                ..SearchField::text("files", &s.original_name)
            }),
    );
    values
}
// Fuzzy comparisons are exhaustive inside an explicit budget; exceeding it is
// an error, never a misleading complete "no matches" result.
fn near(text: &str, query: &str, budget: &mut usize) -> Result<bool, String> {
    let a: Vec<_> = text.chars().collect();
    let b: Vec<_> = query.chars().collect();
    if a.len() > 2048 {
        return Err("fuzzy field exceeds 2048 characters; narrow fields or use contains".into());
    }
    let distance = if b.len() < 8 { 1 } else { 2 };
    for start in 0..a.len() {
        for len in
            b.len().saturating_sub(distance).max(1)..=(b.len() + distance).min(a.len() - start)
        {
            if *budget == 0 {
                return Err(
                    "fuzzy comparison budget exceeded; narrow the scope or use contains".into(),
                );
            }
            *budget -= 1;
            let mut row: Vec<usize> = (0..=b.len()).collect();
            for (i, c) in a[start..start + len].iter().enumerate() {
                let mut next = vec![i + 1];
                for (j, d) in b.iter().enumerate() {
                    next.push(
                        (next[j] + 1)
                            .min(row[j + 1] + 1)
                            .min(row[j] + usize::from(c != d)),
                    );
                }
                row = next;
            }
            if row[b.len()] <= distance {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn excerpt(text: &str, query: &str) -> String {
    let mut norm = String::new();
    let mut map = Vec::new();
    for (index, c) in text.char_indices() {
        for n in normalized(&c.to_string()).chars() {
            norm.push(n);
            map.push(index);
        }
    }
    let start = norm
        .find(query)
        .and_then(|p| map.get(norm[..p].chars().count()).copied())
        .unwrap_or(0);
    let start = text[..start]
        .char_indices()
        .rev()
        .nth(40)
        .map(|(i, _)| i)
        .unwrap_or(0);
    text[start..].chars().take(160).collect()
}
pub struct Hit<'a> {
    pub task: &'a Task,
    pub reasons: Vec<Value>,
    pub score: usize,
    pub index: usize,
}
pub fn select<'a>(
    workspace: &'a Workspace,
    filter: &Filter,
    sort: Sort,
) -> Result<Vec<Hit<'a>>, String> {
    filter.validate()?;
    let zone = offset(filter.utc_offset_minutes)?;
    let query = filter.query.as_deref().map(normalized);
    let mut hits = Vec::new();
    let mut fuzzy_budget = 100_000;
    for (index, task) in workspace.tasks.iter().enumerate() {
        if filter
            .project_id
            .as_ref()
            .is_some_and(|p| p != &task.project_id)
            || match filter.archived {
                Archive::Active => task.archived,
                Archive::Archived => !task.archived,
                Archive::All => false,
            }
        {
            continue;
        }
        let status = serde_json::to_value(&task.status).unwrap();
        let priority = serde_json::to_value(&task.priority).unwrap();
        if filter
            .statuses
            .as_ref()
            .is_some_and(|v| !v.iter().any(|s| status == *s))
            || filter
                .priorities
                .as_ref()
                .is_some_and(|v| !v.iter().any(|s| priority == *s))
        {
            continue;
        }
        if filter.tags.as_ref().is_some_and(|v| {
            !v.iter()
                .all(|s| task.tags.iter().any(|t| normalized(t) == normalized(s)))
        }) {
            continue;
        }
        if filter
            .has_attachments
            .is_some_and(|v| v == task.attachments.is_empty())
            || filter
                .has_images
                .is_some_and(|v| v == task.images.is_empty())
        {
            continue;
        }
        let due = if task.due_date != "9999-12-31" && calendar(&task.due_date).is_ok() {
            Some(task.due_date.as_str())
        } else {
            None
        };
        let completed = completion_day(task, zone);
        if !in_range(due, &filter.due_from, &filter.due_to)
            || !in_range(
                completed.as_deref(),
                &filter.completed_from,
                &filter.completed_to,
            )
        {
            continue;
        }
        let mut reasons = Vec::new();
        let mut score = 0;
        if let Some(q) = &query {
            for entry in field_values(task) {
                let (field, value) = (entry.field, entry.value);
                if filter
                    .fields
                    .as_ref()
                    .is_some_and(|v| !v.iter().any(|s| s == field))
                {
                    continue;
                }
                let text = normalized(value);
                let exact = text == *q;
                let contains = text.contains(q);
                let matched = match filter.match_mode {
                    MatchMode::Exact => exact,
                    MatchMode::Contains => contains,
                    MatchMode::Fuzzy => contains || near(&text, q, &mut fuzzy_budget)?,
                };
                if matched {
                    score += if exact {
                        100
                    } else if contains {
                        50
                    } else {
                        10
                    };
                    if field == "title" {
                        score += 30;
                    }
                    if reasons.len() < 4 {
                        let mut reason = json!({"field":field,"excerpt":excerpt(value,q),"kind":if exact{"exact"}else if contains{"contains"}else{"fuzzy"}});
                        if let Some(id) = entry.item_id {
                            reason["itemId"] = json!(id);
                            reason["completed"] = json!(entry.completed.unwrap());
                        }
                        if let Some(id) = entry.file_id {
                            reason["fileId"] = json!(id);
                        }
                        reasons.push(reason);
                    }
                }
            }
            if reasons.is_empty() {
                continue;
            }
        }
        hits.push(Hit {
            task,
            reasons,
            score,
            index,
        });
    }
    hits.sort_by(|a, b| match sort {
        Sort::Manual => a.index.cmp(&b.index),
        Sort::Relevance => b.score.cmp(&a.score).then(a.index.cmp(&b.index)),
        Sort::DueDate => {
            let key = |t: &Task| {
                if calendar(&t.due_date).is_ok() {
                    t.due_date.clone()
                } else {
                    "9999-12-31".into()
                }
            };
            key(a.task).cmp(&key(b.task)).then(a.index.cmp(&b.index))
        }
        Sort::CompletedAt => b
            .task
            .completed_at
            .as_deref()
            .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
            .cmp(
                &a.task
                    .completed_at
                    .as_deref()
                    .and_then(|v| DateTime::parse_from_rfc3339(v).ok()),
            )
            .then(a.index.cmp(&b.index)),
    });
    Ok(hits)
}
pub fn compact(task: &Task) -> Value {
    json!({"id":task.id,"title":task.title,"projectId":task.project_id,"status":task.status,"priority":task.priority,"version":task.version,"dueDate":task.due_date,"completedAt":task.completed_at,"archived":task.archived,
        "counts":{"subtasks":task.subtasks.len(),"subtasksCompleted":task.subtasks.iter().filter(|s|s.completed).count(),"acceptanceCriteria":task.acceptance_criteria.len(),"criteriaConfirmed":task.acceptance_criteria.iter().filter(|s|s.completed).count(),"attachments":task.attachments.len(),"images":task.images.len()}})
}
pub fn stats(workspace: &Workspace, filter: &Filter, today: Option<&str>) -> Result<Value, String> {
    let zone = offset(filter.utc_offset_minutes)?;
    let day = today.map(str::to_string).unwrap_or_else(|| {
        Local::now()
            .with_timezone(&zone)
            .format("%Y-%m-%d")
            .to_string()
    });
    calendar(&day)?;
    let hits = select(workspace, filter, Sort::Manual)?;
    let mut states = json!({"todo":0,"in_progress":0,"blocked":0,"done":0});
    let (mut overdue, mut due_today, mut completed_today) = (0, 0, 0);
    for hit in &hits {
        let t = hit.task;
        let key = serde_json::to_value(&t.status).unwrap();
        let key = key.as_str().unwrap();
        states[key] = json!(states[key].as_u64().unwrap() + 1);
        if calendar(&t.due_date).is_ok() && t.due_date != "9999-12-31" {
            if t.due_date == day {
                due_today += 1;
            }
            if t.due_date < day && t.status != task_core::TaskStatus::Done {
                overdue += 1;
            }
        }
        if completion_day(t, zone).as_deref() == Some(&day) {
            completed_today += 1;
        }
    }
    Ok(
        json!({"workspaceVersion":workspace.version,"total":hits.len(),"byStatus":states,"overdue":overdue,"dueToday":due_today,"completedToday":completed_today,"completedInRange":if filter.completed_from.is_some()||filter.completed_to.is_some(){Some(hits.len())}else{None},"today":day,"utcOffset":zone.to_string(),"dateSemantics":"inclusive calendar dates at fixed UTC offset; overdue excludes done"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chinese_normalization_and_fuzzy_are_bounded() {
        assert_eq!(normalized(" 完 成 ＡＢＣ \n"), "完成abc");
        assert!(near("完成权限设计", "权限设汁", &mut 10000).unwrap());
        assert!(near(&"长".repeat(2049), "权限", &mut 10000).is_err());
        assert!(near("权限设计", "全限设计", &mut 0).is_err());
    }
}
