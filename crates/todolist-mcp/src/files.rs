use crate::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use file_content::{ReadOptions, ReadOutput};
use std::{collections::HashSet, io::Read, path::PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct TextSubmission {
    pub name: String,
    pub body: String,
}
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ListFilesInput {
    pub task_id: String,
    pub query: Option<String>,
    pub kind: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct RemoveFilesInput {
    pub task_id: String,
    pub file_ids: Vec<String>,
    pub expected_version: u64,
    pub request_id: String,
}
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ReadFileInput {
    pub task_id: String,
    pub file_id: String,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
    pub max_bytes: Option<usize>,
    pub sheet: Option<String>,
    pub range: Option<String>,
    pub original: Option<bool>,
    pub query: Option<String>,
    pub context_lines: Option<usize>,
}
#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SearchFilesInput {
    pub task_ids: Option<Vec<String>>,
    pub project_id: Option<String>,
    pub query: String,
    pub file_name: Option<String>,
    pub include_archived: Option<bool>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}
#[derive(Serialize, Deserialize, Default)]
struct FileCursor {
    fingerprint: String,
    start: usize,
    offset: usize,
    tail: String,
}
#[derive(Serialize, Deserialize, Default)]
struct SearchCursor {
    fingerprint: String,
    index: usize,
    start: usize,
    offset: usize,
    hash: Option<String>,
    tail: String,
    incomplete: bool,
}
pub(crate) fn stage_text(text: &TextSubmission) -> Result<(tempfile::TempDir, PathBuf), String> {
    if text.body.len() > 65536 {
        return Err("text attachment body exceeds 64 KiB UTF-8".into());
    }
    let name = &text.name;
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if name.is_empty()
        || name.chars().count() > 120
        || name
            .chars()
            .any(|c| c.is_control() || "/\\<>:\"|?*".contains(c))
        || name.ends_with(['.', ' '])
        || name.starts_with('.')
        || ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.ends_with(['1', '2', '3', '4', '5', '6', '7', '8', '9']))
    {
        return Err("text attachment name must be a safe single filename".into());
    }
    if !["txt", "md", "json"].contains(
        &Path::new(name)
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
    ) {
        return Err("text submission supports .txt, .md, .json only".into());
    }
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let path = dir.path().join(name);
    fs::write(&path, text.body.as_bytes()).map_err(|e| e.to_string())?;
    Ok((dir, path))
}
fn associated<'a>(
    w: &'a Workspace,
    task_id: &str,
    file_id: &str,
) -> Result<(&'a Task, &'a ManagedFile, &'static str), String> {
    let task = w
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or("task_id not found")?;
    if let Some(f) = task.attachments.iter().find(|f| f.id == file_id) {
        Ok((task, f, "attachments"))
    } else if let Some(f) = task.images.iter().find(|f| f.id == file_id) {
        Ok((task, f, "images"))
    } else {
        Err("file_id is not associated with this task".into())
    }
}
fn checked_path(
    server: &TodoMcpServer,
    file: &ManagedFile,
    collection: &str,
) -> Result<PathBuf, String> {
    if !file.storage_key.starts_with(&format!("{collection}/")) {
        return Err("unsafe managed collection".into());
    }
    let root = server.store.managed_files_root();
    let path = resolve_storage_key(&root, &file.storage_key)?;
    let meta = fs::symlink_metadata(&path).map_err(|_| "managed copy is missing or unavailable")?;
    if task_store_sqlite::managed_files::is_link(&meta) || !meta.is_file() {
        return Err("managed copy must be a regular file without reparse/link redirection".into());
    }
    let canonical = fs::canonicalize(&path).map_err(|_| "managed copy unavailable")?;
    if !canonical.starts_with(fs::canonicalize(root).map_err(|_| "managed root unavailable")?) {
        return Err("managed copy escapes active store root".into());
    }
    Ok(path)
}
#[cfg(windows)]
fn handle_path(file: &fs::File) -> Result<PathBuf, String> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFinalPathNameByHandleW(
            handle: *mut std::ffi::c_void,
            path: *mut u16,
            length: u32,
            flags: u32,
        ) -> u32;
    }
    let mut path = vec![0u16; 32768];
    // Check the opened handle, not the earlier path lookup: an ancestor junction
    // can be swapped between validation and open. The buffer stays live for FFI.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            path.as_mut_ptr(),
            path.len() as u32,
            0,
        )
    } as usize;
    if length == 0 || length >= path.len() {
        return Err("cannot verify opened managed file path".into());
    }
    Ok(PathBuf::from(std::ffi::OsString::from_wide(
        &path[..length],
    )))
}
#[cfg(target_os = "linux")]
fn handle_path(file: &fs::File) -> Result<PathBuf, String> {
    use std::os::fd::AsRawFd;
    fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
        .map_err(|_| "cannot verify opened managed file path".into())
}
#[cfg(not(any(windows, target_os = "linux")))]
fn handle_path(_file: &fs::File) -> Result<PathBuf, String> {
    Err("safe managed handle validation is unsupported on this platform".into())
}
fn read_bytes(
    server: &TodoMcpServer,
    file: &ManagedFile,
    collection: &str,
    budget: usize,
) -> Result<(Vec<u8>, String), String> {
    let root = fs::canonicalize(server.store.managed_files_root())
        .map_err(|_| "managed root unavailable")?;
    let path = checked_path(server, file, collection)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000).share_mode(1);
    }
    let handle = options
        .open(&path)
        .map_err(|_| "managed copy could not be opened safely")?;
    let before = handle
        .metadata()
        .map_err(|_| "managed metadata unavailable")?;
    if !handle_path(&handle)?.starts_with(root) {
        return Err("opened file handle escapes active managed root".into());
    }
    if task_store_sqlite::managed_files::is_link(&before) || !before.is_file() {
        return Err("unsafe managed copy".into());
    }
    if before.len() > file_content::MAX_INPUT_BYTES as u64 {
        return Err("resource_limit: managed file exceeds 16 MiB content input budget".into());
    }
    if before.len() > budget as u64 {
        return Err("page_budget".into());
    }
    let mut bytes = Vec::new();
    let mut limited = handle.take(file_content::MAX_INPUT_BYTES as u64 + 1);
    limited
        .read_to_end(&mut bytes)
        .map_err(|_| "managed content could not be read")?;
    let after = limited
        .get_ref()
        .metadata()
        .map_err(|_| "managed metadata unavailable")?;
    if before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || bytes.len() > file_content::MAX_INPUT_BYTES
    {
        return Err("managed content changed during read; retry".into());
    }
    let hash = format!("{:x}", Sha256::digest(&bytes));
    Ok((bytes, hash))
}
fn validate_query(q: &str) -> Result<String, String> {
    let q = query::normalized(q);
    if q.is_empty() || q.chars().count() > 128 {
        return Err("content query must contain 1 through 128 normalized characters".into());
    }
    Ok(q)
}
fn tail(text: &str, q: &str) -> String {
    let n = query::normalized(text);
    n.chars()
        .rev()
        .take(q.chars().count().saturating_sub(1))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}
fn content_units(
    out: &ReadOutput,
    q: Option<&str>,
    prior_tail: &str,
    context: usize,
) -> Vec<serde_json::Value> {
    let mut indexes = HashSet::new();
    for (i, unit) in out.units.iter().enumerate() {
        let text = if i == 0 {
            format!("{prior_tail}{}", query::normalized(&unit.text))
        } else {
            query::normalized(&unit.text)
        };
        if q.is_none_or(|q| text.contains(q)) {
            for n in i.saturating_sub(context)..=(i + context).min(out.units.len() - 1) {
                indexes.insert(n);
            }
        }
    }
    let mut indexes: Vec<_> = indexes.into_iter().collect();
    indexes.sort_unstable();
    indexes
        .into_iter()
        .map(|i| {
            let u = &out.units[i];
            {
                let mut v = json!({"location":u.location,"text":u.text});
                if i == 0 && !prior_tail.is_empty() {
                    v["normalizedContinuationPrefix"] = json!(prior_tail);
                }
                v
            }
        })
        .collect()
}
impl TodoMcpServer {
    pub(crate) fn list_files_v2(&self, input: ListFilesInput) -> Result<serde_json::Value, String> {
        let w = self.workspace("list_task_files")?;
        let task = w
            .tasks
            .iter()
            .find(|t| t.id == input.task_id)
            .ok_or("task_id not found")?;
        let q = input.query.as_deref().map(validate_query).transpose()?;
        if input
            .kind
            .as_deref()
            .is_some_and(|v| !["attachments", "images"].contains(&v))
        {
            return Err("kind must be attachments or images".into());
        }
        let files: Vec<_> = task
            .attachments
            .iter()
            .map(|f| (f, "attachments"))
            .chain(task.images.iter().map(|f| (f, "images")))
            .filter(|(f, k)| {
                input.kind.as_deref().is_none_or(|v| v == *k)
                    && q.as_ref()
                        .is_none_or(|q| query::normalized(&f.original_name).contains(q))
            })
            .collect();
        let limit = input.limit.unwrap_or(20);
        if !(1..=50).contains(&limit) {
            return Err("file limit must be 1 through 50".into());
        }
        let fingerprint = request_fingerprint(
            &json!({"task":task.id,"version":task.version,"query":q,"kind":input.kind}),
        );
        let start = extensions::page_start(
            input.cursor.as_deref(),
            w.version,
            &fingerprint,
            files.len(),
        )?;
        let end = (start + limit).min(files.len());
        Ok(
            json!({"workspaceVersion":w.version,"taskId":task.id,"taskVersion":task.version,"files":files[start..end].iter().map(|(f,k)|{let mut v=self.file_output(f,k);v["kind"]=json!(k);v}).collect::<Vec<_>>(),"totalCount":files.len(),"nextCursor":extensions::page_cursor(w.version,fingerprint,end,files.len())}),
        )
    }
    pub(crate) fn remove_files_v2(
        &self,
        mut input: RemoveFilesInput,
    ) -> Result<serde_json::Value, String> {
        input.request_id = normalize_required_request_id(input.request_id)?;
        if input.file_ids.is_empty()
            || input.file_ids.len() > 20
            || input.file_ids.iter().collect::<HashSet<_>>().len() != input.file_ids.len()
        {
            return Err("provide 1 through 20 unique associated file ids".into());
        }
        let mut result=self.store.mutate_with_receipt(&self.client.operation("remove_task_files"),&input.request_id,&request_fingerprint(&input),|mut w|{
            let task=w.tasks.iter_mut().find(|t|t.id==input.task_id).ok_or("task_id not found")?;
            if task.version!=input.expected_version{return Err("version_conflict: re-read task version".into());}
            if input.file_ids.iter().any(|id|!task.attachments.iter().chain(&task.images).any(|f|&f.id==id)){return Err("file_id is not associated with this task".into());}
            task.attachments.retain(|f|!input.file_ids.contains(&f.id));task.images.retain(|f|!input.file_ids.contains(&f.id));task.version=task.version.checked_add(1).filter(|v|*v<=MAX_SAFE_INTEGER).ok_or("task version limit reached")?;
            task.push_activity(ActivityItem{id:Uuid::new_v4().to_string(),action:format!("{} 移除托管文件关联",self.client.label()),actor:self.client.actor().into(),at:chrono::Utc::now().to_rfc3339()});
            let result=json!({"task":{"id":task.id,"version":task.version},"removedFileIds":input.file_ids});w.version=w.version.checked_add(1).filter(|v|*v<=MAX_SAFE_INTEGER).ok_or("workspace version limit reached")?;Ok((w,result))
        })?.receipt;
        let cleanup_failed = self.store.cleanup_managed_files().is_err();
        result["cleanupPending"] =
            json!(cleanup_failed || self.store.managed_file_cleanup_pending().unwrap_or(true));
        Ok(result)
    }
    pub(crate) fn read_file_v2(&self, input: ReadFileInput) -> Result<CallToolResult, String> {
        let w = self.workspace("read_task_file")?;
        let (task, file, kind) = associated(&w, &input.task_id, &input.file_id)?;
        let (bytes, hash) = read_bytes(self, file, kind, file_content::MAX_INPUT_BYTES)?;
        let q = input.query.as_deref().map(validate_query).transpose()?;
        let context = input.context_lines.unwrap_or(0);
        if context > 3 {
            return Err("context_lines must be 0 through 3".into());
        }
        let limit = input.limit.unwrap_or(20);
        let maximum = if input.original.unwrap_or(false) {
            262144
        } else {
            65536
        };
        let max_bytes = input
            .max_bytes
            .unwrap_or(if input.original.unwrap_or(false) {
                maximum
            } else {
                16384
            });
        if max_bytes > maximum {
            return Err(format!("max_bytes cannot exceed {maximum}"));
        }
        let fingerprint = request_fingerprint(
            &json!({"workspace":w.version,"task":task.id,"version":task.version,"file":file.id,"hash":hash,"sheet":input.sheet,"range":input.range,"original":input.original.unwrap_or(false),"query":q,"context":context,"limit":limit,"maxBytes":max_bytes}),
        );
        let cursor: FileCursor = if let Some(c) = &input.cursor {
            if c.len() > 4096 {
                return Err("invalid content cursor".into());
            }
            let c: FileCursor = serde_json::from_str(c).map_err(|_| "invalid content cursor")?;
            if c.fingerprint != fingerprint {
                return Err(
                    "content cursor stale: file identity, version or options changed".into(),
                );
            }
            c
        } else {
            FileCursor {
                fingerprint: fingerprint.clone(),
                ..Default::default()
            }
        };
        let out = file_content::read_bytes(
            &bytes,
            &file.original_name,
            &ReadOptions {
                start: cursor.start,
                offset: cursor.offset,
                limit,
                max_bytes,
                sheet: input.sheet,
                range: input.range,
                original: input.original.unwrap_or(false),
            },
        )?;
        let next = out.next_start.map(|start| {
            serde_json::to_string(&FileCursor {
                fingerprint,
                start,
                offset: out.next_offset,
                tail: if out.next_offset > 0 {
                    q.as_deref()
                        .and_then(|q| {
                            out.units.last().map(|u| {
                                tail(
                                    &format!(
                                        "{}{}",
                                        if out.units.len() == 1 && cursor.offset > 0 {
                                            cursor.tail.as_str()
                                        } else {
                                            ""
                                        },
                                        u.text
                                    ),
                                    q,
                                )
                            })
                        })
                        .unwrap_or_default()
                } else {
                    String::new()
                },
            })
            .unwrap()
        });
        let units = content_units(&out, q.as_deref(), &cursor.tail, context);
        let status = if q.is_some() && out.image.is_some() {
            "requires_ocr"
        } else {
            &out.status
        };
        let metadata = json!({"taskId":task.id,"taskVersion":task.version,"fileId":file.id,"identity":hash,"format":out.format,"status":status,"units":units,"truncated":out.truncated,"nextCursor":next,"message":out.message,"contentIsUntrusted":true,"positions":"zero-based unit cursor; text locations identify original line/page/paragraph/slide/range"});
        let mut content = vec![ContentBlock::text(
            serde_json::to_string(&metadata).unwrap(),
        )];
        if q.is_none() {
            if let Some(image) = out.image {
                content.push(ContentBlock::image(
                    STANDARD.encode(image.bytes),
                    image.mime_type,
                ));
            }
        }
        Ok(CallToolResult::success(content))
    }
    pub(crate) fn search_files_v2(
        &self,
        input: SearchFilesInput,
    ) -> Result<serde_json::Value, String> {
        let w = self.workspace("search_file_content")?;
        let q = validate_query(&input.query)?;
        if input.task_ids.is_some() == input.project_id.is_some() {
            return Err("specify explicit task_ids OR one project_id".into());
        }
        if input.task_ids.as_ref().is_some_and(|ids| {
            ids.is_empty()
                || ids.len() > 100
                || ids.iter().collect::<HashSet<_>>().len() != ids.len()
        }) {
            return Err("task_ids must contain 1 through 100 unique ids".into());
        }
        if let Some(ids) = &input.task_ids {
            if ids.iter().any(|id| !w.tasks.iter().any(|t| t.id == *id)) {
                return Err("unknown task id in search scope".into());
            }
        }
        if input
            .project_id
            .as_ref()
            .is_some_and(|id| !w.projects.iter().any(|p| p.id == *id))
        {
            return Err("project_id not found".into());
        }
        let name = input.file_name.as_deref().map(validate_query).transpose()?;
        let tasks: Vec<_> = w
            .tasks
            .iter()
            .filter(|t| {
                (input.include_archived.unwrap_or(input.task_ids.is_some()) || !t.archived)
                    && input
                        .task_ids
                        .as_ref()
                        .is_none_or(|ids| ids.contains(&t.id))
                    && input
                        .project_id
                        .as_ref()
                        .is_none_or(|id| id == &t.project_id)
            })
            .collect();
        if tasks.len() > 100 {
            return Err("search scope exceeds 100 tasks; narrow to explicit ids".into());
        }
        let files: Vec<_> = tasks
            .into_iter()
            .flat_map(|t| {
                t.attachments
                    .iter()
                    .map(move |f| (t, f, "attachments"))
                    .chain(t.images.iter().map(move |f| (t, f, "images")))
            })
            .filter(|(_, f, _)| {
                name.as_ref()
                    .is_none_or(|q| query::normalized(&f.original_name).contains(q))
            })
            .collect();
        if files.len() > 200 {
            return Err("search scope exceeds 200 files; narrow filename or task ids".into());
        }
        let manifest: Vec<_> = files
            .iter()
            .map(|(t, f, k)| {
                let state = checked_path(self, f, k)
                    .ok()
                    .and_then(|p| fs::metadata(p).ok());
                json!([
                    t.id,
                    t.version,
                    f.id,
                    f.storage_key,
                    state.as_ref().map(|m| m.len()),
                    state
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_nanos().to_string())
                ])
            })
            .collect();
        let limit = input.limit.unwrap_or(30);
        if !(1..=50).contains(&limit) {
            return Err("search hit limit must be 1 through 50".into());
        }
        let fingerprint = request_fingerprint(
            &json!({"workspace":w.version,"manifest":manifest,"query":q,"scope":input.task_ids,"project":input.project_id,"name":name,"archive":input.include_archived,"limit":limit}),
        );
        let mut cursor: SearchCursor = if let Some(c) = &input.cursor {
            if c.len() > 4096 {
                return Err("invalid search cursor".into());
            }
            let c: SearchCursor = serde_json::from_str(c).map_err(|_| "invalid search cursor")?;
            if c.fingerprint != fingerprint || c.index > files.len() {
                return Err(
                    "search cursor stale: workspace, scope, file metadata or options changed"
                        .into(),
                );
            }
            c
        } else {
            SearchCursor {
                fingerprint,
                ..Default::default()
            }
        };
        let (mut hits, mut issues) = (Vec::new(), Vec::new());
        let (mut calls, mut scanned_bytes, mut units_scanned) = (0, 0usize, 0usize);
        while cursor.index < files.len() && calls < 4 && hits.len() < limit {
            let (task, file, kind) = files[cursor.index];
            let (bytes, hash) = match read_bytes(self, file, kind, 32 * 1024 * 1024 - scanned_bytes)
            {
                Ok(v) => v,
                Err(e) if e == "page_budget" => break,
                Err(e) => {
                    issues.push(json!({"taskId":task.id,"fileId":file.id,"status":"unavailable","message":e}));
                    cursor.incomplete = true;
                    cursor.index += 1;
                    cursor.start = 0;
                    cursor.offset = 0;
                    cursor.hash = None;
                    cursor.tail.clear();
                    calls += 1;
                    continue;
                }
            };
            if cursor.hash.as_ref().is_some_and(|old| old != &hash) {
                return Err("search cursor stale: active file bytes changed".into());
            }
            scanned_bytes += bytes.len();
            calls += 1;
            let out = file_content::read_bytes(
                &bytes,
                &file.original_name,
                &ReadOptions {
                    start: cursor.start,
                    offset: cursor.offset,
                    limit: (limit - hits.len()).min(20),
                    max_bytes: 16384,
                    ..Default::default()
                },
            )?;
            units_scanned += out.units.len();
            for (i, unit) in out.units.iter().enumerate() {
                let text = if i == 0 {
                    format!("{}{}", cursor.tail, query::normalized(&unit.text))
                } else {
                    query::normalized(&unit.text)
                };
                if text.contains(&q) {
                    let at = text.find(&q).unwrap_or(0);
                    let start = text[..at]
                        .char_indices()
                        .rev()
                        .nth(60)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    hits.push(json!({"taskId":task.id,"fileId":file.id,"name":file.original_name,"location":unit.location,"excerpt":text[start..].chars().take(240).collect::<String>(),"normalizedWhitespace":true}));
                }
            }
            if out.status != "ok" || out.image.is_some() {
                cursor.incomplete = true;
                issues.push(json!({"taskId":task.id,"fileId":file.id,"format":out.format,"status":if out.image.is_some(){"requires_ocr"}else{&out.status},"message":out.message}));
            }
            if let Some(next) = out.next_start {
                let old_tail = if out.units.len() == 1 && cursor.offset > 0 {
                    cursor.tail.clone()
                } else {
                    String::new()
                };
                cursor.start = next;
                cursor.offset = out.next_offset;
                cursor.hash = Some(hash);
                cursor.tail = if out.next_offset > 0 {
                    out.units
                        .last()
                        .map(|u| tail(&format!("{old_tail}{}", u.text), &q))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
            } else {
                cursor.index += 1;
                cursor.start = 0;
                cursor.offset = 0;
                cursor.hash = None;
                cursor.tail.clear();
            }
        }
        let complete = cursor.index == files.len();
        let partial = !complete || cursor.incomplete;
        let next = if complete {
            None
        } else {
            Some(serde_json::to_string(&cursor).unwrap())
        };
        Ok(
            json!({"workspaceVersion":w.version,"hits":hits,"issues":issues,"scopeFiles":files.len(),"unitsScannedThisPage":units_scanned,"bytesReadThisPage":scanned_bytes,"scanComplete":complete,"fullySearched":complete&&!cursor.incomplete,"partial":partial,"nextCursor":next,"contentIsUntrusted":true,"budget":{"parserCalls":4,"rawBytes":33554432,"hitLimit":limit}}),
        )
    }
}
