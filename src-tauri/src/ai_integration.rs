//! Explicit, local client adapters. Status reads never configure a client or test its connection.
use crate::codex_integration::{self as codex, atomic_write, backup_file, IntegrationPaths};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use task_store_sqlite::managed_files::is_link;
use task_store_sqlite::storage_path::{APP_IDENTIFIER, IS_PRODUCTION};
use tempfile::NamedTempFile;
use toml_edit::{value, Array, DocumentMut};

const CORE_SKILL: &str = include_str!("../../.agents/skills/todolist-mcp/SKILL.md");
const MARKER: &str = ".todolist-managed";
const RECEIPT: &str = ".todolist-integration.json";
const DSH_BEGIN: &str = "# BEGIN TodoList managed MCP (app.todolist.desktop)\n";
const DSH_END: &str = "# END TodoList managed MCP (app.todolist.desktop)\n";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Client {
    Codex,
    ClaudeCode,
    DeepseekHarness,
    Antigravity,
}

impl Client {
    fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude_code",
            Self::DeepseekHarness => "deepseek_harness",
            Self::Antigravity => "antigravity",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code (CC)",
            Self::DeepseekHarness => "DeepSeek Harness (DSH)",
            Self::Antigravity => "Google Antigravity",
        }
    }
    fn marker(self) -> String {
        format!("app.todolist.desktop/{}\n", self.id())
    }
    fn skill(self) -> String {
        let adapter = match self {
            Self::Codex => "",
            Self::ClaudeCode => "\n## Claude Code adapter\n\nDiscover this personal Skill as /todolist-mcp. MCP tools are named mcp__todolist__<tool>; use the actual discovered tools. A returned localPath belongs to the MCP host; read it only with a separately permitted tool on that same filesystem. read_task_file delivers standard MCP image content; do not equate an unrendered image or requires_ocr with verified text. Respect Claude Code tool approvals and managed restrictions. This Skill declares no allowed-tools and grants no permissions. Reconnect MCP or restart Claude Code after configuring; use /reload-skills when adding a new Skill root.\n",
            Self::DeepseekHarness => "\n## DeepSeek Harness adapter\n\nDiscover todolist-mcp through the harness Skill tools and its user Skill root. MCP tools are named mcp__todolist__<tool>; use the actual discovered tools. A returned localPath belongs to the MCP host; read it only with a separately permitted tool on that same filesystem. read_task_file uses standard MCP image content; verify the active client actually exposes the image, and do not infer OCR or rendering from registration. Keep harness permission approvals in force. The active profile must resolve @deepseek-ai/dsh-mcp-client. Restart the harness after changing the home patch; registration alone does not prove that the active profile loaded the plugin or connected.\n",
            Self::Antigravity => "\n## Google Antigravity adapter\n\nOpen /todolist-mcp or discover it in Agent Skills. Use the actual tools exposed by the todolist MCP server, subject to client and workspace permissions. A returned localPath belongs to the MCP host; read it only with a separately permitted tool on that filesystem. Restart the MCP server or reload Antigravity after configuration. Only a successful tool call verifies connectivity.\n",
        };
        format!("{CORE_SKILL}{adapter}")
    }
    fn reconnect(self) -> &'static str {
        match self {
        Self::Codex => "重新连接 MCP 或重启 Codex。",
        Self::ClaudeCode => "重新连接 MCP 或重启 Claude Code；新增 Skill 根目录后可运行 /reload-skills。客户端与组织权限仍然适用。",
        Self::DeepseekHarness => "重启 Harness，并在活动 profile 中确认 @deepseek-ai/dsh-mcp-client 可解析；项目 Skill 和后续 patch 可覆盖用户配置。TodoList 不自动安装 Harness 插件或授予工具权限。",
            Self::Antigravity => "在 Antigravity 中重启 todolist MCP 或重新打开客户端，再执行只读 list_projects 验证连接。App 与 CLI 共用全局 Skill。",
    }
    }
    fn json_registration_fields(self) -> Option<&'static [&'static str]> {
        match self {
            Self::ClaudeCode => Some(&["type", "command", "args"]),
            Self::Antigravity => Some(&["command", "args", "serverUrl"]),
            _ => None,
        }
    }
    fn registration_matches(self, actual: &Value, expected: &Value) -> bool {
        let Some(fields) = self.json_registration_fields() else {
            return actual == expected;
        };
        expected.get("command").and_then(Value::as_str).is_some()
            && fields.iter().all(|&key| {
                // Antigravity may omit the optional empty args when saving its JSON.
                if self == Self::Antigravity && key == "args" {
                    let empty = |value: Option<&Value>| {
                        value.is_none_or(|v| v.as_array().is_some_and(Vec::is_empty))
                    };
                    return empty(actual.get(key)) && empty(expected.get(key));
                }
                actual.get(key) == expected.get(key)
            })
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Locations {
    pub config_path: Option<String>,
    pub skill_path: Option<String>,
}

#[derive(Clone)]
struct Paths {
    client: Client,
    config: PathBuf,
    skill: PathBuf,
    executable: PathBuf,
    detected: bool,
    detection: String,
    manual: bool,
}
impl Paths {
    fn codex(&self) -> IntegrationPaths {
        IntegrationPaths {
            codex_config: self.config.clone(),
            skill_directory: self.skill.clone(),
            mcp_executable: self.executable.clone(),
        }
    }
    fn entry(&self) -> Value {
        if self.client == Client::Antigravity {
            // Released sidecars already accept no arguments and attribute those writes to generic AI.
            return json!({"command":self.executable.to_string_lossy(), "args":[]});
        }
        json!({"type":"stdio", "command":self.executable.to_string_lossy(), "args":["--client", self.client.id()]})
    }
    fn block(&self) -> String {
        // JSON strings are valid YAML double-quoted scalars, including Windows paths.
        let command = serde_json::to_string(&self.executable.to_string_lossy()).unwrap();
        format!("{DSH_BEGIN}- insert:\n    - id: todolist-mcp\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        serverName: todolist\n        transport: stdio\n        command: {command}\n        args: ['--client', 'deepseek_harness']\n{DSH_END}")
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    client: Client,
    label: String,
    detected: bool,
    detection: String,
    state: String,
    configured: bool,
    can_configure: bool,
    can_remove: bool,
    managed_migration: bool,
    reason: String,
    pending_updates: Vec<String>,
    action_result: String,
    updated_items: Vec<String>,
    config_path: String,
    skill_path: String,
    mcp_command: String,
    mcp_state: String,
    skill_state: String,
    runtime_health: &'static str,
    message: String,
    reconnect: String,
}
impl Status {
    fn new(paths: &Paths) -> Self {
        Self {
            client: paths.client,
            label: paths.client.label().into(),
            detected: paths.detected,
            detection: paths.detection.clone(),
            state: "not_configured".into(),
            configured: false,
            can_configure: true,
            can_remove: false,
            managed_migration: false,
            reason: "missing_config".into(),
            pending_updates: vec![],
            action_result: String::new(),
            updated_items: vec![],
            config_path: paths.config.to_string_lossy().into_owned(),
            skill_path: paths.skill.to_string_lossy().into_owned(),
            mcp_command: paths.executable.to_string_lossy().into_owned(),
            mcp_state: "missing".into(),
            skill_state: "missing".into(),
            runtime_health: "unverified",
            message: "尚未注册 MCP，也未安装 Skill。".into(),
            reconnect: paths.client.reconnect().into(),
        }
    }
    fn conflict(&mut self, reason: &str, message: &str) {
        self.state = "conflict".into();
        self.reason = reason.into();
        self.message = message.into();
        self.can_configure = false;
        self.can_remove = false;
        self.configured = false;
    }
}

fn safe_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("请选择不含 .. 的绝对路径。".into());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if is_link(&meta) => {
                return Err("配置路径包含链接；已保持原样，请选择真实路径。".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}
fn read(path: &Path) -> Result<Option<String>, String> {
    safe_path(path)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
        Ok(meta) if !meta.is_file() => Err("目标不是普通文件，已保持原样。".into()),
        Ok(_) => fs::read_to_string(path)
            .map(Some)
            .map_err(|e| e.to_string()),
    }
}
fn json_document(content: Option<&str>) -> Result<Value, String> {
    // Duplicate JSON keys are ambiguous ownership and must not be silently lost
    // when preserving the rest of the client's user configuration.
    let value = serde_json::from_str::<UniqueJson>(content.unwrap_or("{}"))
        .map_err(|e| format!("客户端 JSON 配置无效：{e}"))?;
    let value = value.0;
    if !value.is_object() || value.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err("客户端配置及 mcpServers 必须是 JSON 对象。".into());
    }
    Ok(value)
}

struct UniqueJson(Value);
impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON without duplicate object keys")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<UniqueJson, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<UniqueJson, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<UniqueJson, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<UniqueJson, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<UniqueJson, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<UniqueJson, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<UniqueJson, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut items: A,
            ) -> Result<UniqueJson, A::Error> {
                let mut values = vec![];
                while let Some(value) = items.next_element::<UniqueJson>()? {
                    values.push(value.0);
                }
                Ok(UniqueJson(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut items: A,
            ) -> Result<UniqueJson, A::Error> {
                let mut map = serde_json::Map::new();
                while let Some(key) = items.next_key::<String>()? {
                    if map.contains_key(&key) {
                        return Err(serde::de::Error::custom("duplicate JSON object key"));
                    }
                    map.insert(key, items.next_value::<UniqueJson>()?.0);
                }
                Ok(UniqueJson(Value::Object(map)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    client: Client,
    config_path: String,
    entry: Value,
}
fn receipt(paths: &Paths) -> Result<Option<Receipt>, String> {
    read(&paths.skill.join(RECEIPT))?
        .map(|s| serde_json::from_str(&s).map_err(|e| format!("TodoList 托管记录无效：{e}")))
        .transpose()
}
fn managed_skill(paths: &Paths) -> Result<(bool, bool), String> {
    safe_path(&paths.skill)?;
    if !paths.skill.exists() {
        return Ok((false, false));
    }
    if !paths.skill.is_dir() {
        return Err("Skill 路径不是目录。".into());
    }
    let managed = read(&paths.skill.join(MARKER))?.is_some_and(|s| s == paths.client.marker());
    let current =
        managed && read(&paths.skill.join("SKILL.md"))?.is_some_and(|s| s == paths.client.skill());
    Ok((managed, current))
}
fn dsh_range(content: &str) -> Result<Option<std::ops::Range<usize>>, String> {
    let starts: Vec<_> = content.match_indices(DSH_BEGIN).map(|(i, _)| i).collect();
    let ends: Vec<_> = content.match_indices(DSH_END).map(|(i, _)| i).collect();
    match (starts.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([start], [end])
            if start < end && (*start == 0 || content.as_bytes()[start - 1] == b'\n') =>
        {
            Ok(Some(*start..end + DSH_END.len()))
        }
        _ => Err("DSH 托管块标记不完整或重复，已保持原样。".into()),
    }
}
fn dsh_has_todolist(value: &serde_yaml_ng::Value) -> bool {
    use serde_yaml_ng::Value as Y;
    match value {
        Y::Mapping(map) => map.iter().any(|(key, value)| {
            key.as_str() == Some("todolist-mcp")
                || (matches!(key.as_str(), Some("id" | "serverName"))
                    && matches!(value.as_str(), Some("todolist-mcp" | "todolist")))
                || dsh_has_todolist(value)
        }),
        Y::Sequence(items) => items.iter().any(dsh_has_todolist),
        Y::Tagged(tag) => dsh_has_todolist(&tag.value),
        _ => false,
    }
}
fn dsh_document(content: &str) -> Result<serde_yaml_ng::Value, String> {
    let value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(content).map_err(|e| format!("DSH patch 无效：{e}"))?;
    if !value.is_sequence() && !value.is_null() {
        return Err("DSH patch 顶层必须是 patch 列表。".into());
    }
    Ok(value)
}
fn dsh_external(content: &str, range: Option<std::ops::Range<usize>>) -> String {
    let mut external = content.to_owned();
    if let Some(range) = range {
        external.replace_range(range, "");
    }
    external
}
fn dsh_insert(external: &str, block: &str) -> Result<String, String> {
    let value = dsh_document(external)?;
    let mut result = external.to_string();
    if value.as_sequence().is_some_and(|s| s.is_empty()) {
        // Preserve surrounding comments when replacing the empty-list placeholder.
        let mut offset = 0;
        let index = result
            .split_inclusive('\n')
            .find_map(|line| {
                let trimmed = line.trim_start();
                let start = offset + line.len() - trimmed.len();
                offset += line.len();
                (trimmed.starts_with("[]")
                    && (trimmed[2..].trim().is_empty() || trimmed[2..].trim().starts_with('#')))
                .then_some(start)
            })
            .ok_or("无法安全修改 DSH 空列表格式，请选择独立 patch 文件。")?;
        result.replace_range(index..index + 2, "");
    } else if value.is_sequence()
        && external
            .lines()
            .find(|line| {
                !line.trim().is_empty()
                    && !line.trim_start().starts_with('#')
                    && line.trim() != "---"
            })
            .is_some_and(|line| line.trim_start().starts_with('['))
    {
        return Err(
            "DSH flow 列表无法保留原文追加，请选择独立 patch 文件或改用普通 patch 列表。".into(),
        );
    }
    if result.lines().any(|line| line.trim() == "...") {
        return Err("DSH patch 含文档结束标记，无法安全追加；请选择独立 patch 文件。".into());
    }
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result.push_str(block);
    dsh_document(&result)?;
    Ok(result)
}
fn inspect(paths: &Paths) -> Result<Status, String> {
    safe_path(&paths.config)?;
    safe_path(&paths.skill)?;
    if paths.client == Client::Codex {
        return inspect_codex(paths);
    }
    let mut status = Status::new(paths);
    let (managed, current) = managed_skill(paths)?;
    status.skill_state = if current {
        "current"
    } else if managed {
        "update_available"
    } else if paths.skill.exists() {
        "conflict"
    } else {
        "missing"
    }
    .into();
    if paths.skill.exists() && !managed {
        status.conflict(
            "unmanaged_skill",
            "同名 Skill 不属于 TodoList 管理，请选择其他路径。",
        );
        return Ok(status);
    }
    let record = if managed { receipt(paths)? } else { None };
    if record.as_ref().is_some_and(|r| {
        r.client != paths.client || r.config_path != paths.config.to_string_lossy()
    }) {
        status.conflict(
            "receipt_mismatch",
            "Skill 的托管记录属于其他客户端或配置路径，已保持原样。",
        );
        return Ok(status);
    }
    let text = read(&paths.config)?;
    let entry = if paths.client.json_registration_fields().is_some() {
        json_document(text.as_deref())?
            .get("mcpServers")
            .and_then(|s| s.get("todolist"))
            .cloned()
    } else {
        let text = text.as_deref().unwrap_or("");
        dsh_document(text)?;
        let range = dsh_range(text)?;
        let external = dsh_external(text, range.clone());
        if dsh_has_todolist(&dsh_document(&external)?) {
            status.mcp_state = "conflict".into();
            status.conflict(
                "unknown_config",
                "DSH 中存在其他 TodoList 条目或 patch，已保持原样。",
            );
            return Ok(status);
        }
        if range.is_none() {
            dsh_insert(&external, &paths.block())?;
        }
        range.map(|r| Value::String(text[r].to_string()))
    };
    if let Some(ref entry) = entry {
        let owned = record
            .as_ref()
            .is_some_and(|r| paths.client.registration_matches(entry, &r.entry));
        if !owned {
            status.mcp_state = "conflict".into();
            status.conflict(
                "unknown_config",
                "同名 MCP 条目未被可靠识别为 TodoList 托管，已保持原样。",
            );
            return Ok(status);
        }
        let expected = if paths.client.json_registration_fields().is_some() {
            paths.entry()
        } else {
            Value::String(paths.block())
        };
        let matches = paths.client.registration_matches(entry, &expected);
        status.mcp_state = if matches {
            "registered"
        } else {
            "path_migration"
        }
        .into();
        status.managed_migration = !matches;
    }
    status.can_remove = entry.is_some() || managed;
    if status.managed_migration {
        status.state = "partial".into();
        status.reason = "path_migration".into();
        status.message = "托管 MCP 使用旧安装路径，可在确认后迁移。".into();
        status.pending_updates.push("MCP 安装路径".into());
    } else if entry.is_some() && current {
        status.state = "configured".into();
        status.configured = true;
        status.reason = "up_to_date".into();
        status.message = "MCP 注册与 Skill 磁盘文件已同步；连接状态尚未验证。".into();
    } else if entry.is_some() && managed && paths.skill.join("SKILL.md").is_file() {
        status.state = "update_available".into();
        status.reason = "managed_update".into();
        status.message = "可同步新版 Skill 使用说明。".into();
        status.pending_updates.push("新版使用说明".into());
    } else if entry.is_some() || managed {
        status.state = "partial".into();
        status.reason = "missing_component".into();
        status.message = "MCP 注册或 Skill 文件缺失，可重新配置。".into();
    }
    missing_client(paths, &mut status);
    Ok(status)
}
fn missing_client(paths: &Paths, status: &mut Status) {
    if !paths.detected && !paths.manual && status.state == "not_configured" {
        status.state = "client_missing".into();
        status.can_configure = false;
        status.reason = "client_missing".into();
        status.message =
            "未发现客户端或其配置目录；安装客户端后刷新，或指定配置与 Skill 路径。".into();
    }
    if !paths.executable.is_file() {
        status.can_configure = false;
        status.reason = "sidecar_missing".into();
        status.message = "当前安装缺少 MCP 程序，无法配置。".into();
    }
}
fn inspect_codex(paths: &Paths) -> Result<Status, String> {
    let original = codex::inspect(&paths.codex())?;
    let mut status = Status::new(paths);
    status.state = original.state;
    status.configured = original.configured;
    status.can_configure = original.can_configure;
    status.managed_migration = original.managed_migration;
    status.reason = original.reason;
    status.message = original.message;
    status.pending_updates = original.pending_updates;
    let document = read(&paths.config)?
        .unwrap_or_default()
        .parse::<DocumentMut>()
        .map_err(|e| e.to_string())?;
    let server = document.get("mcp_servers").and_then(|s| s.get("todolist"));
    let managed = read(&paths.skill.join(MARKER))?.is_some_and(|s| s == "app.todolist.desktop\n");
    let skill = read(&paths.skill.join("SKILL.md"))?;
    status.skill_state = if skill.as_deref() == Some(CORE_SKILL) && managed {
        "current"
    } else if managed && skill.is_some() {
        "update_available"
    } else if managed || !paths.skill.exists() {
        "missing"
    } else {
        "conflict"
    }
    .into();
    status.mcp_state = if server.is_none() {
        "missing"
    } else if status.reason == "unknown_config" {
        "conflict"
    } else if status.managed_migration {
        "path_migration"
    } else {
        "registered"
    }
    .into();
    status.can_remove = status.state != "conflict" && (server.is_some() || managed);
    if status.can_configure && server.is_some() {
        let args = server.and_then(|s| s.get("args"));
        let values = args
            .and_then(|a| a.as_array())
            .map(|a| a.iter().map(|v| v.as_str()).collect::<Vec<_>>());
        if args.is_some()
            && values
                .as_ref()
                .is_none_or(|v| !v.is_empty() && v != &[Some("--client"), Some("codex")])
        {
            status.mcp_state = "conflict".into();
            status.conflict(
                "unknown_arguments",
                "MCP 有未知启动参数，已保留；请手动确认来源参数。",
            );
        } else if values.as_ref().is_none_or(|v| v.is_empty()) {
            status.configured = false;
            status.mcp_state = "update_available".into();
            if !status.managed_migration {
                status.state = "update_available".into();
            }
            status.pending_updates.push("客户端来源参数".into());
            status.message = "需要同步客户端来源参数与托管集成内容。".into();
        }
    }
    missing_client(paths, &mut status);
    Ok(status)
}

// Recovery is a regular JSON file, never another directory containing SKILL.md.
// Keep only owned Skill files here; full client-config backups stay by that config.
fn skill_recovery(
    paths: &Paths,
    files: &BTreeMap<String, String>,
    id: uuid::Uuid,
) -> Result<PathBuf, String> {
    let name = paths
        .skill
        .file_name()
        .ok_or("Skill 路径缺少目录名。")?
        .to_string_lossy();
    let destination = paths
        .skill
        .with_file_name(format!("{name}.todolist-recovery-{id}.json"));
    safe_path(&destination)?;
    let content = serde_json::to_string_pretty(&json!({
        "format":"todolist-skill-recovery", "version":1, "client":paths.client, "files":files
    }))
    .map_err(|e| e.to_string())?;
    match read(&destination)? {
        Some(existing) if existing == content => return Ok(destination),
        Some(_) => return Err("恢复文件已存在且内容不同，已保留原件。".into()),
        None => {}
    }
    let parent = destination.parent().ok_or("恢复文件缺少父目录。")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temporary
        .write_all(content.as_bytes())
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    // A name collision must never overwrite an unknown recovery file.
    temporary
        .persist_noclobber(&destination)
        .map_err(|e| e.error.to_string())?;
    Ok(destination)
}

fn legacy_skill_backup(
    paths: &Paths,
    directory: &Path,
) -> Result<BTreeMap<String, String>, String> {
    safe_path(directory)?;
    if !fs::symlink_metadata(directory)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        return Err("不是普通备份目录".into());
    }
    let marker = if paths.client == Client::Codex {
        "app.todolist.desktop\n".into()
    } else {
        paths.client.marker()
    };
    // Check ownership before reading a similarly named sibling's old Skill.
    if read(&directory.join(MARKER))?.as_ref() != Some(&marker) {
        return Err("缺少可确认的管理标记".into());
    }
    let allowed = if paths.client == Client::Codex {
        vec!["SKILL.md", MARKER, ".todolist-command"]
    } else {
        vec!["SKILL.md", MARKER, RECEIPT]
    };
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "文件名无法确认")?;
        if !allowed.contains(&name.as_str()) {
            return Err("含未知文件或目录".into());
        }
        let content = read(&entry.path())?.ok_or("备份文件已变化")?;
        files.insert(name, content);
    }
    if files.get(MARKER) != Some(&marker) || !files.contains_key("SKILL.md") {
        return Err("缺少可确认的管理标记或 Skill".into());
    }
    if let Some(receipt) = files.get(RECEIPT) {
        let receipt: Receipt = serde_json::from_str(receipt).map_err(|_| "托管记录无效")?;
        if receipt.client != paths.client {
            return Err("托管记录属于其他客户端".into());
        }
    }
    Ok(files)
}

// Called only after an explicit configure/update/remove succeeds. A retirement
// failure must not turn a committed integration change into a failed operation.
fn retire_legacy_skill_backups(paths: &Paths, status: &mut Status) {
    let Some(parent) = paths.skill.parent() else {
        return;
    };
    let prefix = format!(
        "{}.todolist-backup-",
        paths
            .skill
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    );
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            status
                .message
                .push_str(" 旧 Skill 备份目录无法读取，已保持原样。");
            return;
        }
    };
    let mut retired = 0;
    for entry in entries {
        let Ok(entry) = entry else {
            status
                .message
                .push_str(" 有旧备份目录项无法读取，已保持原样。");
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(suffix) = name.strip_prefix(&prefix) else {
            continue;
        };
        let result = (|| {
            let id = uuid::Uuid::parse_str(suffix).map_err(|_| "名称无法确认归属")?;
            if id.to_string() != suffix || id.get_version_num() != 4 {
                return Err("名称无法确认归属".to_string());
            }
            let directory = entry.path();
            let files = legacy_skill_backup(paths, &directory)?;
            skill_recovery(paths, &files, id)?;
            // Recheck the entire small, known file set after the recovery is
            // durable. Unknown/new content is never recursively removed.
            if legacy_skill_backup(paths, &directory)? != files {
                return Err("备份内容已变化".into());
            }
            safe_path(&directory.join("SKILL.md"))?;
            fs::remove_file(directory.join("SKILL.md")).map_err(|e| e.to_string())?;
            for (name, content) in &files {
                if name != "SKILL.md" {
                    let path = directory.join(name);
                    if read(&path)?.as_ref() != Some(content) {
                        return Err("备份内容已变化".into());
                    }
                    fs::remove_file(path).map_err(|e| e.to_string())?;
                }
            }
            fs::remove_dir(directory).map_err(|e| e.to_string())?;
            Ok(())
        })();
        match result {
            Ok(()) => retired += 1,
            Err(reason) => status
                .message
                .push_str(&format!(" 旧备份 {name} 已保留或未完全退役：{reason}。")),
        }
    }
    if retired > 0 {
        status
            .updated_items
            .push(format!("旧 Skill 备份转为恢复文件（{retired}份）"));
        status.message.push_str(&format!(
            " 已将{retired}份旧 Skill 备份转为非 Skill 的 JSON 恢复文件。"
        ));
    }
}

// Snapshot only the selected client's owned files. On failure, undo this operation and keep backups.
fn transaction<T>(
    paths: &Paths,
    preferences: Option<&Path>,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let mut files = vec![
        paths.config.clone(),
        paths.skill.join("SKILL.md"),
        paths.skill.join(MARKER),
        paths.skill.join(if paths.client == Client::Codex {
            ".todolist-command"
        } else {
            RECEIPT
        }),
    ];
    if let Some(path) = preferences {
        files.push(path.to_path_buf());
    }
    files.sort();
    files.dedup();
    let snapshots: Vec<_> = files
        .into_iter()
        .map(|p| {
            let old = read(&p)?;
            Ok((p, old))
        })
        .collect::<Result<_, String>>()?;
    let mut skill_files = BTreeMap::new();
    for (path, content) in &snapshots {
        if let Some(content) = content {
            if path == &paths.config || preferences == Some(path.as_path()) {
                backup_file(path)?;
            } else {
                skill_files.insert(
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    content.clone(),
                );
            }
        }
    }
    if !skill_files.is_empty() {
        skill_recovery(paths, &skill_files, uuid::Uuid::new_v4())?;
    }
    let existed = paths.skill.exists();
    match operation() {
        Ok(result) => Ok(result),
        Err(error) => {
            let mut failures = vec![];
            for (path, old) in snapshots.into_iter().rev() {
                if read(&path).ok().flatten() == old {
                    continue;
                }
                let result = match old {
                    Some(content) => atomic_write(&path, &content),
                    None if path.exists() => fs::remove_file(&path).map_err(|e| e.to_string()),
                    None => Ok(()),
                };
                if let Err(reason) = result {
                    failures.push(reason);
                }
            }
            if !existed {
                let _ = fs::remove_dir(&paths.skill);
            }
            if failures.is_empty() {
                Err(format!("{error}；本次文件修改已回退，备份已保留。"))
            } else {
                Err(format!("{error}；回退未完成：{}", failures.join("；")))
            }
        }
    }
}
#[cfg(test)]
fn configure(paths: &Paths) -> Result<Status, String> {
    configure_and_remember(paths, None)
}
fn configure_and_remember(
    paths: &Paths,
    preferences: Option<(&Path, &str)>,
) -> Result<Status, String> {
    let before = inspect(paths)?;
    if !before.can_configure {
        return Err(before.message);
    }
    let preferences_unchanged = match preferences {
        Some((path, content)) => read(path)?.as_deref() == Some(content),
        None => true,
    };
    if before.configured && preferences_unchanged {
        let mut status = before;
        status.action_result = "configured".into();
        retire_legacy_skill_backups(paths, &mut status);
        return Ok(status);
    }
    let mut status = transaction(paths, preferences.map(|(p, _)| p), || {
        let mut items = vec![];
        if paths.client == Client::Codex {
            let result = codex::configure(&paths.codex())?;
            items.extend(result.updated_items);
            // A path migration is command-only in the legacy adapter. Finish Skill synchronization separately.
            if before.managed_migration {
                items.extend(codex::configure(&paths.codex())?.updated_items);
            }
            let mut document = read(&paths.config)?
                .unwrap_or_default()
                .parse::<DocumentMut>()
                .map_err(|e| e.to_string())?;
            let mut args = Array::new();
            args.push("--client");
            args.push("codex");
            let needs_args = document["mcp_servers"]["todolist"]
                .get("args")
                .and_then(|a| a.as_array())
                .is_none_or(|a| a.is_empty());
            if needs_args {
                document["mcp_servers"]["todolist"]["args"] = value(args);
                atomic_write(&paths.config, &document.to_string())?;
                items.push("客户端来源参数".into());
            }
        } else {
            let old = read(&paths.config)?;
            let fields = paths.client.json_registration_fields();
            let entry = if fields.is_some() {
                paths.entry()
            } else {
                Value::String(paths.block())
            };
            let next = if let Some(fields) = fields {
                let mut doc = json_document(old.as_deref())?;
                let servers = doc
                    .as_object_mut()
                    .unwrap()
                    .entry("mcpServers")
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .unwrap();
                if let Some(server) = servers.get_mut("todolist") {
                    if paths.client.registration_matches(server, &entry) {
                        old.as_deref().unwrap_or_default().to_string()
                    } else {
                        // Update the owned launch properties in place. Keep the client's tool rules.
                        for &key in fields {
                            if let Some(value) = entry.get(key) {
                                server[key] = value.clone();
                            }
                        }
                        format!("{}\n", serde_json::to_string_pretty(&doc).unwrap())
                    }
                } else {
                    servers.insert("todolist".into(), entry.clone());
                    format!("{}\n", serde_json::to_string_pretty(&doc).unwrap())
                }
            } else {
                let text = old.as_deref().unwrap_or("");
                if let Some(range) = dsh_range(text)? {
                    let mut text = text.to_string();
                    text.replace_range(range, &paths.block());
                    text
                } else {
                    dsh_insert(text, &paths.block())?
                }
            };
            if old.as_deref() != Some(&next) {
                atomic_write(&paths.config, &next)?;
                items.push(
                    if before.managed_migration {
                        "MCP 安装路径"
                    } else {
                        "MCP 注册"
                    }
                    .into(),
                );
            }
            if read(&paths.skill.join("SKILL.md"))?.as_deref() != Some(&paths.client.skill()) {
                atomic_write(&paths.skill.join("SKILL.md"), &paths.client.skill())?;
                items.push("TodoList Skill".into());
            }
            atomic_write(&paths.skill.join(MARKER), &paths.client.marker())?;
            let record = Receipt {
                client: paths.client,
                config_path: paths.config.to_string_lossy().into_owned(),
                entry,
            };
            atomic_write(
                &paths.skill.join(RECEIPT),
                &serde_json::to_string_pretty(&record).unwrap(),
            )?;
        }
        if let Some((path, content)) = preferences {
            atomic_write(path, content).map_err(|e| format!("保存集成路径设置失败：{e}"))?;
        }
        let mut status = inspect(paths)?;
        status.action_result = if before.managed_migration {
            "migrated"
        } else if before.state == "update_available" {
            "updated"
        } else {
            "configured"
        }
        .into();
        items.sort();
        items.dedup();
        status.updated_items = items;
        Ok(status)
    })?;
    retire_legacy_skill_backups(paths, &mut status);
    Ok(status)
}
fn remove(paths: &Paths) -> Result<Status, String> {
    let before = inspect(paths)?;
    if !before.can_remove {
        return Err("未找到可安全移除的 TodoList 托管集成。".into());
    }
    let mut status = transaction(paths, None, || {
        if paths.client == Client::Codex {
            codex::remove_config(&paths.codex())?;
            codex::remove_skill(&paths.codex())?;
        } else {
            if let Some(text) = read(&paths.config)? {
                let next = if paths.client.json_registration_fields().is_some() {
                    let mut doc = json_document(Some(&text))?;
                    let removed = doc
                        .get_mut("mcpServers")
                        .and_then(Value::as_object_mut)
                        .and_then(|servers| servers.remove("todolist"));
                    if removed.is_some() {
                        format!("{}\n", serde_json::to_string_pretty(&doc).unwrap())
                    } else {
                        text.clone()
                    }
                } else {
                    let mut external = dsh_external(&text, dsh_range(&text)?);
                    if dsh_document(&external)?.is_null() {
                        external.push_str("\n[]\n");
                    }
                    external
                };
                if text != next {
                    atomic_write(&paths.config, &next)?;
                }
            }
            for name in ["SKILL.md", MARKER, RECEIPT] {
                let path = paths.skill.join(name);
                if path.exists() {
                    fs::remove_file(path).map_err(|e| e.to_string())?;
                }
            }
            let _ = fs::remove_dir(&paths.skill);
        }
        let mut status = inspect(paths)?;
        status.action_result = "removed".into();
        status.updated_items = vec!["TodoList MCP 注册与托管 Skill".into()];
        Ok(status)
    })?;
    retire_legacy_skill_backups(paths, &mut status);
    Ok(status)
}

fn expand_home_path(path: PathBuf, home: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    if value == "~" {
        home.to_path_buf()
    } else if let Some(relative) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        home.join(relative)
    } else {
        path
    }
}
fn env_path(name: &str, fallback: PathBuf, home: &Path) -> PathBuf {
    std::env::var_os(name)
        .filter(|s| !s.to_string_lossy().trim().is_empty())
        .map(|value| expand_home_path(PathBuf::from(value), home))
        .unwrap_or(fallback)
}
fn executable_in_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .flat_map(|dir| {
                [
                    dir.join(name),
                    dir.join(format!("{name}.exe")),
                    dir.join(format!("{name}.cmd")),
                ]
            })
            .find(|p| p.is_file())
    })
}
fn defaults(client: Client, home: &Path) -> (PathBuf, PathBuf) {
    match client {
        Client::Codex => (
            env_path("CODEX_HOME", home.join(".codex"), home).join("config.toml"),
            home.join(".agents/skills/todolist-mcp"),
        ),
        Client::ClaudeCode => {
            let root = env_path("CLAUDE_CONFIG_DIR", home.join(".claude"), home);
            let config = if std::env::var_os("CLAUDE_CONFIG_DIR")
                .is_some_and(|s| !s.to_string_lossy().trim().is_empty())
            {
                root.join(".claude.json")
            } else {
                home.join(".claude.json")
            };
            (config, root.join("skills/todolist-mcp"))
        }
        Client::DeepseekHarness => {
            let root = env_path("DSH_HOME", home.join(".dsh"), home);
            (
                root.join("cordis.patch.yml"),
                root.join("skills/todolist-mcp"),
            )
        }
        Client::Antigravity => {
            let root = home.join(".gemini/config");
            (
                root.join("mcp_config.json"),
                root.join("skills/todolist-mcp"),
            )
        }
    }
}
fn detect(client: Client, home: &Path, config: &Path) -> (bool, String) {
    let name = match client {
        Client::Codex => "codex",
        Client::ClaudeCode => "claude",
        Client::DeepseekHarness => "dsh",
        Client::Antigravity => "agy",
    };
    let executable = executable_in_path(name)
        .or_else(|| {
            let p = home.join(".local/bin").join(format!("{name}.exe"));
            p.is_file().then_some(p)
        })
        .or_else(|| {
            (client == Client::Antigravity)
                .then(|| executable_in_path("antigravity"))
                .flatten()
        });
    if let Some(path) = executable {
        return (true, format!("发现启动程序：{}", path.display()));
    }
    if config.exists() || config.parent().is_some_and(|p| p.is_dir() && p != home) {
        return (true, "发现配置目录；未确认客户端启动程序。".into());
    }
    (false, "未发现启动程序或配置目录。".into())
}
fn preferences_path() -> Result<PathBuf, String> {
    Ok(dirs::data_local_dir()
        .ok_or("无法定位 TodoList 配置目录")?
        .join(APP_IDENTIFIER)
        .join("ai-integration-locations.json"))
}
fn preferences() -> Result<BTreeMap<Client, Locations>, String> {
    read(&preferences_path()?)?
        .map(|s| serde_json::from_str(&s).map_err(|e| format!("集成路径设置无效：{e}")))
        .transpose()
        .map(|s| s.unwrap_or_default())
}
fn paths(client: Client, locations: Option<Locations>) -> Result<Paths, String> {
    let home = dirs::home_dir().ok_or("无法定位用户目录")?;
    let stored = preferences()?.remove(&client).unwrap_or_default();
    let locations = locations.unwrap_or(stored);
    let manual = locations.config_path.is_some() || locations.skill_path.is_some();
    let (config, skill) = defaults(client, &home);
    let config = locations.config_path.map(PathBuf::from).unwrap_or(config);
    let skill = locations.skill_path.map(PathBuf::from).unwrap_or(skill);
    safe_path(&config)?;
    safe_path(&skill)?;
    if config.starts_with(&skill) {
        return Err("MCP 配置文件不能放在托管 Skill 目录内。".into());
    }
    let (detected, detection) = detect(client, &home, &config);
    Ok(Paths {
        client,
        config,
        skill,
        executable: codex::resolve_mcp_executable()?,
        detected,
        detection,
        manual,
    })
}
fn status_for(paths: &Paths) -> Status {
    inspect(paths).unwrap_or_else(|error| {
        let mut status = Status::new(paths);
        status.state = "error".into();
        status.can_configure = false;
        status.reason = "read_error".into();
        status.mcp_state = "error".into();
        status.skill_state = "error".into();
        status.message = error;
        status
    })
}
#[tauri::command]
pub fn ai_integration_status(
    client: Client,
    locations: Option<Locations>,
) -> Result<Status, String> {
    if !IS_PRODUCTION {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let paths = Paths {
            client,
            config: root.join(".codex/config.toml"),
            skill: root.join(".agents/skills/todolist-mcp"),
            executable: root.join("scripts/start-mcp.mjs"),
            detected: false,
            detection: "开发预览不检测或读取用户客户端配置。".into(),
            manual: false,
        };
        let mut status = Status::new(&paths);
        status.state = "development".into();
        status.can_configure = false;
        status.mcp_state = "development".into();
        status.skill_state = "development".into();
        status.reason = "development".into();
        status.message =
            "开发版只使用项目内 todolist_dev 与独立开发库；日常集成请在安装版配置。".into();
        return Ok(status);
    }
    paths(client, locations).map(|p| status_for(&p))
}
#[tauri::command]
pub fn configure_ai_integration(
    client: Client,
    locations: Option<Locations>,
) -> Result<Status, String> {
    if !IS_PRODUCTION {
        return Err("开发版不能更改全局 AI 集成。".into());
    }
    let paths = paths(client, locations)?;
    // Validate preference persistence before changing client files.
    let path = preferences_path()?;
    let mut saved = preferences()?;
    saved.insert(
        client,
        Locations {
            config_path: Some(paths.config.to_string_lossy().into_owned()),
            skill_path: Some(paths.skill.to_string_lossy().into_owned()),
        },
    );
    configure_and_remember(
        &paths,
        Some((&path, &serde_json::to_string_pretty(&saved).unwrap())),
    )
}
#[tauri::command]
pub fn remove_ai_integration(
    client: Client,
    locations: Option<Locations>,
) -> Result<Status, String> {
    if !IS_PRODUCTION {
        return Err("开发版不能移除全局 AI 集成。".into());
    }
    remove(&paths(client, locations)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_client_skill_explains_managed_local_paths_without_granting_access() {
        for client in [
            Client::Codex,
            Client::ClaudeCode,
            Client::DeepseekHarness,
            Client::Antigravity,
        ] {
            let skill = client.skill();
            assert!(skill.contains("localPath"), "{}", client.label());
            assert!(skill.contains("MCP host"), "{}", client.label());
            assert!(skill.contains("permission"), "{}", client.label());
        }
    }

    fn fixture(root: &Path, client: Client) -> Paths {
        let directory = root.join(client.id());
        fs::create_dir_all(&directory).unwrap();
        let executable = root.join("todolist-mcp.exe");
        fs::write(&executable, b"fixture").unwrap();
        Paths {
            client,
            config: directory.join(match client {
                Client::Codex => "config.toml",
                Client::ClaudeCode => ".claude.json",
                Client::DeepseekHarness => "cordis.patch.yml",
                Client::Antigravity => "mcp_config.json",
            }),
            skill: directory.join("skills/todolist-mcp"),
            executable,
            detected: true,
            detection: "fixture".into(),
            manual: false,
        }
    }

    #[test]
    fn antigravity_lifecycle_keeps_tool_rules_and_uses_no_sidecar_argument() {
        let root = tempfile::tempdir().unwrap();
        let paths = fixture(root.path(), Client::Antigravity);
        let (config, skill) = defaults(Client::Antigravity, root.path());
        assert_eq!(config, root.path().join(".gemini/config/mcp_config.json"));
        assert_eq!(
            skill,
            root.path().join(".gemini/config/skills/todolist-mcp")
        );
        let unrelated = json!({
            "appearance": "light",
            "mcpServers": {"custom": {"serverUrl": "https://example.invalid/mcp", "disabled": true}}
        });
        fs::write(&paths.config, unrelated.to_string()).unwrap();
        assert!(!paths.skill.exists());
        assert_eq!(inspect(&paths).unwrap().state, "not_configured");
        assert!(!paths.skill.exists());

        assert_eq!(configure(&paths).unwrap().runtime_health, "unverified");
        let mut configured = json_document(read(&paths.config).unwrap().as_deref()).unwrap();
        assert_eq!(
            configured["mcpServers"]["todolist"],
            json!({
                "command": paths.executable.to_string_lossy(), "args": []
            })
        );
        let server = configured["mcpServers"]["todolist"]
            .as_object_mut()
            .unwrap();
        server.remove("args");
        server.insert("enabledTools".into(), json!(["list_projects"]));
        server.insert("disabledTools".into(), json!(["create_task"]));
        server.insert("tools".into(), json!({"list_projects": {"eager": true}}));
        server.insert("disabled".into(), json!(true));
        let client_format = configured.to_string();
        fs::write(&paths.config, &client_format).unwrap();
        assert!(configure(&paths).unwrap().updated_items.is_empty());
        assert!(recovery_files(&paths).is_empty());

        fs::write(paths.skill.join("SKILL.md"), "old managed skill").unwrap();
        let updated = configure(&paths).unwrap();
        assert_eq!(updated.action_result, "updated");
        assert_eq!(updated.updated_items, ["TodoList Skill"]);
        assert_eq!(fs::read_to_string(&paths.config).unwrap(), client_format);
        assert_eq!(recovery_files(&paths).len(), 1);

        let mut migrated = paths.clone();
        migrated.executable = root.path().join("new-todolist-mcp.exe");
        fs::write(&migrated.executable, b"fixture").unwrap();
        assert!(inspect(&migrated).unwrap().managed_migration);
        assert_eq!(configure(&migrated).unwrap().action_result, "migrated");
        let mut saved = json_document(read(&paths.config).unwrap().as_deref()).unwrap();
        assert_eq!(
            saved["mcpServers"]["todolist"]["command"],
            migrated.executable.to_string_lossy().as_ref()
        );
        assert_eq!(
            saved["mcpServers"]["todolist"]["enabledTools"],
            json!(["list_projects"])
        );
        assert_eq!(
            saved["mcpServers"]["todolist"]["disabledTools"],
            json!(["create_task"])
        );
        assert_eq!(
            saved["mcpServers"]["todolist"]["tools"],
            json!({"list_projects": {"eager": true}})
        );
        assert_eq!(saved["mcpServers"]["todolist"]["disabled"], json!(true));

        saved["mcpServers"]["todolist"]["serverUrl"] = json!("https://example.invalid/replacement");
        fs::write(&paths.config, saved.to_string()).unwrap();
        assert!(!inspect(&migrated).unwrap().can_remove);
        assert!(configure(&migrated).is_err());
        assert_eq!(
            fs::read_to_string(&paths.config).unwrap(),
            saved.to_string()
        );
        saved["mcpServers"]["todolist"]
            .as_object_mut()
            .unwrap()
            .remove("serverUrl");
        fs::write(&paths.config, saved.to_string()).unwrap();
        assert_eq!(remove(&migrated).unwrap().action_result, "removed");
        assert_eq!(
            json_document(read(&paths.config).unwrap().as_deref()).unwrap(),
            unrelated
        );
        assert!(!paths.skill.join("SKILL.md").exists());
    }

    #[test]
    fn antigravity_same_name_without_receipt_stays_unowned() {
        let root = tempfile::tempdir().unwrap();
        let paths = fixture(root.path(), Client::Antigravity);
        let original = json!({"mcpServers": {"todolist": paths.entry()}}).to_string();
        fs::write(&paths.config, &original).unwrap();
        let status = inspect(&paths).unwrap();
        assert_eq!(status.reason, "unknown_config");
        assert!(!status.can_configure && !status.can_remove);
        assert!(configure(&paths).is_err() && remove(&paths).is_err());
        assert_eq!(fs::read_to_string(&paths.config).unwrap(), original);
        assert!(!paths.skill.exists());
    }
    fn legacy_backup_fixture(paths: &Paths) -> PathBuf {
        let directory = paths.skill.with_file_name(format!(
            "{}.todolist-backup-{}",
            paths.skill.file_name().unwrap().to_string_lossy(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).unwrap();
        for name in ["SKILL.md", MARKER, RECEIPT, ".todolist-command"] {
            if let Some(content) = read(&paths.skill.join(name)).unwrap() {
                fs::write(directory.join(name), content).unwrap();
            }
        }
        directory
    }
    fn discoverable_skills(root: &Path) -> Vec<PathBuf> {
        let mut result = vec![];
        if root.exists() {
            for entry in fs::read_dir(root).unwrap() {
                let path = entry.unwrap().path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                if is_link(&metadata) {
                    continue;
                }
                if metadata.is_dir() {
                    result.extend(discoverable_skills(&path));
                } else if path.file_name().is_some_and(|name| name == "SKILL.md") {
                    result.push(path);
                }
            }
        }
        result.sort();
        result
    }
    fn recovery_files(paths: &Paths) -> Vec<PathBuf> {
        let mut result = fs::read_dir(paths.skill.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".todolist-recovery-")
            })
            .collect::<Vec<_>>();
        result.sort();
        result
    }

    #[test]
    fn updates_and_removal_never_leave_discoverable_recovery_skills_for_any_client() {
        for client in [Client::Codex, Client::ClaudeCode, Client::DeepseekHarness] {
            let root = tempfile::tempdir().unwrap();
            let paths = fixture(root.path(), client);
            configure(&paths).unwrap();
            for revision in 0..3 {
                let old = format!("old owned Skill {revision}");
                fs::write(paths.skill.join("SKILL.md"), &old).unwrap();
                let updated = configure(&paths).unwrap();
                assert_eq!(updated.action_result, "updated");
                assert_eq!(
                    read(&paths.skill.join("SKILL.md")).unwrap().unwrap(),
                    client.skill()
                );
                assert_eq!(
                    discoverable_skills(root.path()),
                    vec![paths.skill.join("SKILL.md")]
                );
                assert!(recovery_files(&paths).iter().any(|p| {
                    let value: Value =
                        serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap();
                    value["files"]["SKILL.md"] == old && value.get("config").is_none()
                }));
            }
            let recovery_count = recovery_files(&paths).len();
            let config_backups = fs::read_dir(paths.config.parent().unwrap())
                .unwrap()
                .count();
            let preferences = root.path().join("integration-locations.json");
            fs::write(&preferences, "{}").unwrap();
            for _ in 0..3 {
                assert!(
                    configure_and_remember(&paths, Some((&preferences, "{}")))
                        .unwrap()
                        .configured
                );
            }
            assert_eq!(recovery_files(&paths).len(), recovery_count);
            assert_eq!(
                fs::read_dir(paths.config.parent().unwrap())
                    .unwrap()
                    .count(),
                config_backups
            );
            remove(&paths).unwrap();
            assert!(discoverable_skills(root.path()).is_empty());
            assert_eq!(recovery_files(&paths).len(), recovery_count + 1);
        }
    }

    #[test]
    fn explicit_updates_and_removal_retire_only_owned_legacy_backups() {
        for client in [Client::Codex, Client::ClaudeCode, Client::DeepseekHarness] {
            let root = tempfile::tempdir().unwrap();
            let paths = fixture(root.path(), client);
            configure(&paths).unwrap();
            let one = legacy_backup_fixture(&paths);
            let two = legacy_backup_fixture(&paths);
            let before = fs::read(one.join("SKILL.md")).unwrap();
            let status = configure(&paths).unwrap(); // Current integration is a no-op; retirement is explicit.
            assert!(status.configured);
            assert!(!one.exists() && !two.exists());
            assert_eq!(
                discoverable_skills(root.path()),
                vec![paths.skill.join("SKILL.md")]
            );
            assert_eq!(recovery_files(&paths).len(), 2);
            for recovery in recovery_files(&paths) {
                let value: Value =
                    serde_json::from_str(&fs::read_to_string(recovery).unwrap()).unwrap();
                assert_eq!(value["format"], "todolist-skill-recovery");
                assert_eq!(
                    value["files"]["SKILL.md"].as_str().unwrap().as_bytes(),
                    before
                );
            }
            let three = legacy_backup_fixture(&paths);
            remove(&paths).unwrap();
            assert!(!three.exists());
            assert!(discoverable_skills(root.path()).is_empty());
        }
    }

    #[test]
    fn uncertain_legacy_names_markers_and_contents_are_preserved_with_notice() {
        for client in [Client::Codex, Client::ClaudeCode, Client::DeepseekHarness] {
            let root = tempfile::tempdir().unwrap();
            let paths = fixture(root.path(), client);
            configure(&paths).unwrap();
            let unknown = legacy_backup_fixture(&paths);
            fs::write(unknown.join("user-file.txt"), "keep me").unwrap();
            let unowned = legacy_backup_fixture(&paths);
            fs::write(unowned.join(MARKER), "not TodoList").unwrap();
            let malformed = paths
                .skill
                .with_file_name("todolist-mcp.todolist-backup-not-a-uuid");
            fs::create_dir_all(&malformed).unwrap();
            fs::write(malformed.join("SKILL.md"), "keep unknown name").unwrap();
            let other = paths.skill.with_file_name("another-skill");
            fs::create_dir_all(&other).unwrap();
            fs::write(other.join("SKILL.md"), "unrelated Skill").unwrap();
            let status = configure(&paths).unwrap();
            assert!(status.configured);
            for kept in [&unknown, &unowned, &malformed] {
                assert!(kept.join("SKILL.md").exists());
                assert!(status
                    .message
                    .contains(kept.file_name().unwrap().to_str().unwrap()));
            }
            assert_eq!(
                fs::read_to_string(unknown.join("user-file.txt")).unwrap(),
                "keep me"
            );
            assert_eq!(
                fs::read_to_string(other.join("SKILL.md")).unwrap(),
                "unrelated Skill"
            );
            assert!(!status.message.contains("another-skill"));
            remove(&paths).unwrap();
            assert!(unknown.join("SKILL.md").exists() && unowned.join("SKILL.md").exists());
        }
    }

    #[test]
    fn a_conflicting_recovery_file_cannot_fail_configuration_or_destroy_the_original() {
        let root = tempfile::tempdir().unwrap();
        let paths = fixture(root.path(), Client::ClaudeCode);
        configure(&paths).unwrap();
        let legacy = legacy_backup_fixture(&paths);
        let suffix = legacy
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .strip_prefix("todolist-mcp.todolist-backup-")
            .unwrap();
        let recovery = paths
            .skill
            .with_file_name(format!("todolist-mcp.todolist-recovery-{suffix}.json"));
        fs::write(&recovery, "unowned recovery contents").unwrap();
        let status = configure(&paths).unwrap();
        assert!(status.configured);
        assert!(status.message.contains("内容不同"));
        assert!(legacy.join("SKILL.md").exists());
        assert_eq!(
            fs::read_to_string(recovery).unwrap(),
            "unowned recovery contents"
        );
    }

    #[cfg(windows)]
    #[test]
    fn held_legacy_skill_reports_retirement_failure_without_rolling_back_success() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let paths = fixture(root.path(), Client::ClaudeCode);
        configure(&paths).unwrap();
        let legacy = legacy_backup_fixture(&paths);
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(legacy.join("SKILL.md"))
            .unwrap();
        fs::write(paths.skill.join("SKILL.md"), "old current skill").unwrap();
        let status = configure(&paths).unwrap();
        assert!(status.configured && status.action_result == "updated");
        assert!(legacy.join("SKILL.md").exists());
        assert!(status.message.contains("未完全退役"));
        let count = recovery_files(&paths).len();
        assert!(configure(&paths).unwrap().configured);
        assert_eq!(recovery_files(&paths).len(), count);
        drop(held);
        assert!(configure(&paths).unwrap().configured);
        assert!(!legacy.exists());
        assert_eq!(recovery_files(&paths).len(), count);
    }

    #[cfg(windows)]
    #[test]
    fn legacy_junctions_and_linked_children_are_preserved_without_following_them() {
        let root = tempfile::tempdir().unwrap();
        let paths = fixture(root.path(), Client::Codex);
        configure(&paths).unwrap();
        let outside = root.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("SKILL.md"), "outside Skill").unwrap();
        fs::write(outside.join(MARKER), "app.todolist.desktop\n").unwrap();
        let junction = paths.skill.with_file_name(format!(
            "todolist-mcp.todolist-backup-{}",
            uuid::Uuid::new_v4()
        ));
        let child = legacy_backup_fixture(&paths);
        fs::remove_file(child.join(MARKER)).unwrap();
        for link in [&junction, &child.join(MARKER)] {
            let result = std::process::Command::new("cmd.exe")
                .args(["/D", "/C", "mklink", "/J"])
                .arg(link)
                .arg(&outside)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        let status = configure(&paths).unwrap();
        assert!(status.configured && status.message.contains("链接"));
        assert!(junction.join("SKILL.md").exists() && child.join("SKILL.md").exists());
        assert_eq!(
            fs::read_to_string(outside.join("SKILL.md")).unwrap(),
            "outside Skill"
        );
        let mut unsafe_paths = paths.clone();
        unsafe_paths.skill = junction.join("current");
        assert!(configure(&unsafe_paths).is_err());
        fs::remove_dir(&junction).unwrap();
        fs::remove_dir(child.join(MARKER)).unwrap();
    }
    #[test]
    fn independent_lifecycle_preserves_unrelated_settings_and_backups() {
        let root = tempfile::tempdir().unwrap();
        let codex = fixture(root.path(), Client::Codex);
        let cc = fixture(root.path(), Client::ClaudeCode);
        let dsh = fixture(root.path(), Client::DeepseekHarness);
        fs::write(
            &codex.config,
            "theme = 'dark'\n[mcp_servers.other]\ncommand = 'other'\n",
        )
        .unwrap();
        fs::write(&cc.config, r#"{"theme":"dark","permissions":{"deny":["mcp__todolist__update_task"]},"mcpServers":{"other":{"command":"other"}}}"#).unwrap();
        let external = "# user comment\n- insert:\n    - id: other\n      name: '@other/plugin'\n      config:\n        token: !!js process.env.MY_TOKEN\n";
        fs::write(&dsh.config, external).unwrap();
        for paths in [&codex, &cc, &dsh] {
            assert_eq!(inspect(paths).unwrap().state, "not_configured");
            let status = configure(paths).unwrap();
            assert!(
                status.configured,
                "{}: {}",
                paths.client.id(),
                status.message
            );
            assert_eq!(status.runtime_health, "unverified");
            assert_eq!(status.mcp_state, "registered");
            assert_eq!(status.skill_state, "current");
        }
        let cc_saved = read(&cc.config).unwrap().unwrap();
        let codex_saved = read(&codex.config).unwrap().unwrap();
        assert!(codex_saved.contains("'dark'"));
        assert!(codex_saved.contains("--client"));
        let cc_doc = json_document(Some(&cc_saved)).unwrap();
        assert_eq!(
            cc_doc["permissions"]["deny"][0],
            "mcp__todolist__update_task"
        );
        assert_eq!(cc_doc["mcpServers"]["other"]["command"], "other");
        assert!(read(&dsh.config).unwrap().unwrap().starts_with(external));
        remove(&dsh).unwrap();
        assert_eq!(read(&dsh.config).unwrap().as_deref(), Some(external));
        assert_eq!(read(&cc.config).unwrap().unwrap(), cc_saved);
        assert_eq!(read(&codex.config).unwrap().unwrap(), codex_saved);
        for paths in [&codex, &cc] {
            remove(paths).unwrap();
            assert!(!paths.skill.exists());
            assert_eq!(inspect(paths).unwrap().state, "not_configured");
            configure(paths).unwrap();
        }
        assert!(fs::read_dir(root.path().join("claude_code"))
            .unwrap()
            .any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("todolist-backup")));
    }
    #[test]
    fn new_client_updates_migration_and_user_fields_survive() {
        for client in [Client::ClaudeCode, Client::DeepseekHarness] {
            let root = tempfile::tempdir().unwrap();
            let paths = fixture(root.path(), client);
            configure(&paths).unwrap();
            fs::write(paths.skill.join("SKILL.md"), "old managed Skill").unwrap();
            assert_eq!(inspect(&paths).unwrap().state, "update_available");
            assert_eq!(configure(&paths).unwrap().action_result, "updated");
            if client == Client::ClaudeCode {
                let mut doc = json_document(read(&paths.config).unwrap().as_deref()).unwrap();
                doc["mcpServers"]["todolist"]["env"] = json!({"MY_SETTING":"keep"});
                fs::write(&paths.config, serde_json::to_string(&doc).unwrap()).unwrap();
            }
            let mut moved = paths.clone();
            moved.executable = root.path().join("new/todolist-mcp.exe");
            fs::create_dir_all(moved.executable.parent().unwrap()).unwrap();
            fs::write(&moved.executable, b"new fixture").unwrap();
            assert!(inspect(&moved).unwrap().managed_migration);
            assert_eq!(configure(&moved).unwrap().action_result, "migrated");
            assert!(inspect(&moved).unwrap().configured);
            if client == Client::ClaudeCode {
                assert_eq!(
                    json_document(read(&paths.config).unwrap().as_deref()).unwrap()["mcpServers"]
                        ["todolist"]["env"]["MY_SETTING"],
                    "keep"
                );
            }
            remove(&moved).unwrap();
        }
    }
    #[test]
    fn unknown_entries_skills_and_modified_owned_entries_are_never_overwritten() {
        for client in [Client::ClaudeCode, Client::DeepseekHarness] {
            let root = tempfile::tempdir().unwrap();
            let paths = fixture(root.path(), client);
            let unknown = if client == Client::ClaudeCode {
                "{\"mcpServers\":{\"todolist\":{\"command\":\"other\"}}}"
            } else {
                "- insert:\n    - id: other\n      name: other\n      config:\n        serverName: todolist\n"
            };
            fs::write(&paths.config, unknown).unwrap();
            assert_eq!(inspect(&paths).unwrap().state, "conflict");
            assert!(configure(&paths).is_err());
            assert!(remove(&paths).is_err());
            assert_eq!(read(&paths.config).unwrap().unwrap(), unknown);
            fs::remove_file(&paths.config).unwrap();
            fs::create_dir_all(&paths.skill).unwrap();
            fs::write(paths.skill.join("SKILL.md"), "unowned").unwrap();
            assert_eq!(inspect(&paths).unwrap().skill_state, "conflict");
            assert!(configure(&paths).is_err());
            fs::remove_file(paths.skill.join("SKILL.md")).unwrap();
            fs::remove_dir(&paths.skill).unwrap();
            configure(&paths).unwrap();
            let modified = if client == Client::ClaudeCode {
                let mut doc = json_document(read(&paths.config).unwrap().as_deref()).unwrap();
                doc["mcpServers"]["todolist"]["args"] = json!(["--other"]);
                serde_json::to_string(&doc).unwrap()
            } else {
                read(&paths.config)
                    .unwrap()
                    .unwrap()
                    .replace("transport: stdio", "transport: http")
            };
            fs::write(&paths.config, &modified).unwrap();
            assert_eq!(inspect(&paths).unwrap().state, "conflict");
            assert!(remove(&paths).is_err());
            assert_eq!(read(&paths.config).unwrap().unwrap(), modified);
        }
    }
    #[test]
    fn missing_skill_component_is_repairable_and_unknown_files_survive_removal() {
        for client in [
            Client::ClaudeCode,
            Client::DeepseekHarness,
            Client::Antigravity,
        ] {
            let root = tempfile::tempdir().unwrap();
            let paths = fixture(root.path(), client);
            configure(&paths).unwrap();
            fs::remove_file(paths.skill.join("SKILL.md")).unwrap();
            assert_eq!(inspect(&paths).unwrap().state, "partial");
            configure(&paths).unwrap();
            fs::write(paths.skill.join("notes.txt"), "user content").unwrap();
            remove(&paths).unwrap();
            assert_eq!(
                read(&paths.skill.join("notes.txt")).unwrap().unwrap(),
                "user content"
            );
            assert!(!paths.skill.join("SKILL.md").exists());
        }
    }
    #[test]
    fn rollback_restores_exact_files_and_unique_backups_are_outside_skill() {
        for client in [
            Client::Codex,
            Client::ClaudeCode,
            Client::DeepseekHarness,
            Client::Antigravity,
        ] {
            let root = tempfile::tempdir().unwrap();
            let paths = fixture(root.path(), client);
            configure(&paths).unwrap();
            let old = read(&paths.config).unwrap().unwrap();
            let skill = read(&paths.skill.join("SKILL.md")).unwrap().unwrap();
            let result: Result<(), String> = transaction(&paths, None, || {
                atomic_write(&paths.config, "changed")?;
                fs::remove_file(paths.skill.join("SKILL.md")).unwrap();
                Err("fixture failure".into())
            });
            assert!(result.unwrap_err().contains("已回退"));
            assert_eq!(read(&paths.config).unwrap().unwrap(), old);
            assert_eq!(read(&paths.skill.join("SKILL.md")).unwrap().unwrap(), skill);
            assert!(inspect(&paths).unwrap().configured);
            assert_eq!(
                discoverable_skills(root.path()),
                vec![paths.skill.join("SKILL.md")]
            );
            let recoveries = recovery_files(&paths);
            assert_eq!(recoveries.len(), 1);
            let recovery: Value =
                serde_json::from_str(&fs::read_to_string(&recoveries[0]).unwrap()).unwrap();
            assert_eq!(recovery["files"]["SKILL.md"], skill);
            assert!(recovery.get("config").is_none());
            assert!(fs::read_dir(&paths.skill).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("backup")));
        }
    }
    #[test]
    fn codex_legacy_origin_upgrade_retains_restricted_tools_and_migration() {
        let root = tempfile::tempdir().unwrap();
        let paths = fixture(root.path(), Client::Codex);
        configure(&paths).unwrap();
        let mut doc = read(&paths.config)
            .unwrap()
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        doc["mcp_servers"]["todolist"]
            .as_table_mut()
            .unwrap()
            .remove("args");
        let mut tools = Array::new();
        tools.push("get_task");
        doc["mcp_servers"]["todolist"]["enabled_tools"] = value(tools);
        fs::write(&paths.config, doc.to_string()).unwrap();
        assert_eq!(inspect(&paths).unwrap().state, "update_available");
        configure(&paths).unwrap();
        let doc = read(&paths.config)
            .unwrap()
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(
            doc["mcp_servers"]["todolist"]["enabled_tools"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let mut moved = paths.clone();
        moved.executable = root.path().join("new.exe");
        fs::write(&moved.executable, b"new").unwrap();
        assert!(inspect(&moved).unwrap().managed_migration);
        configure(&moved).unwrap();
        assert!(inspect(&moved).unwrap().configured);
        let mut doc = read(&paths.config)
            .unwrap()
            .unwrap()
            .parse::<DocumentMut>()
            .unwrap();
        let mut args = Array::new();
        args.push("--custom");
        doc["mcp_servers"]["todolist"]["args"] = value(args);
        fs::write(&paths.config, doc.to_string()).unwrap();
        assert_eq!(inspect(&moved).unwrap().state, "conflict");
        assert!(remove(&moved).is_err());
    }
    #[test]
    fn malformed_configs_markers_and_unsafe_paths_fail_without_writes() {
        let root = tempfile::tempdir().unwrap();
        for (client, invalid) in [
            (Client::ClaudeCode, "{broken"),
            (Client::ClaudeCode, "{\"mcpServers\":{},\"mcpServers\":{}}"),
            (Client::ClaudeCode, "{\"mcpServers\":[]} "),
            (Client::DeepseekHarness, "- insert: [broken"),
            (Client::DeepseekHarness, "plugins: {}"),
            (Client::DeepseekHarness, DSH_BEGIN),
        ] {
            let paths = fixture(root.path(), client);
            fs::write(&paths.config, invalid).unwrap();
            assert!(inspect(&paths).is_err());
            assert!(configure(&paths).is_err());
            assert_eq!(read(&paths.config).unwrap().unwrap(), invalid);
            assert!(!paths.skill.exists());
        }
        assert!(safe_path(Path::new("relative/config.json")).is_err());
        assert!(safe_path(&root.path().join("../outside")).is_err());
        assert_eq!(
            expand_home_path(PathBuf::from("~/custom"), root.path()),
            root.path().join("custom")
        );
        assert_eq!(
            expand_home_path(PathBuf::from("~\\custom"), root.path()),
            root.path().join("custom")
        );
    }
    #[test]
    #[cfg(windows)]
    fn failure_saving_locations_rolls_back_the_whole_selected_integration() {
        let root = tempfile::tempdir().unwrap();
        let paths = fixture(root.path(), Client::ClaudeCode);
        let preferences = root.path().join("locations.json");
        fs::write(&preferences, "{}").unwrap();
        let mut permissions = fs::metadata(&preferences).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&preferences, permissions).unwrap();
        let result = configure_and_remember(&paths, Some((&preferences, "{\"new\":true}")));
        let mut permissions = fs::metadata(&preferences).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&preferences, permissions).unwrap();
        assert!(result.unwrap_err().contains("已回退"));
        assert!(!paths.config.exists());
        assert!(!paths.skill.exists());
        assert_eq!(read(&preferences).unwrap().as_deref(), Some("{}"));
    }
    #[test]
    fn dsh_empty_list_comments_and_tagged_aliases_keep_their_text() {
        let root = tempfile::tempdir().unwrap();
        let paths = fixture(root.path(), Client::DeepseekHarness);
        fs::write(&paths.config, "# keep header []\n[] # keep tail\n").unwrap();
        configure(&paths).unwrap();
        let text = read(&paths.config).unwrap().unwrap();
        assert!(text.contains("# keep header"));
        assert!(text.contains("# keep tail"));
        remove(&paths).unwrap();
        assert!(dsh_document(&read(&paths.config).unwrap().unwrap())
            .unwrap()
            .as_sequence()
            .unwrap()
            .is_empty());
        let text="- insert:\n    - id: other\n      config: &settings\n        expression: !!js 'process.env.KEY'\n    - id: other-two\n      config: *settings\n";
        let inserted = dsh_insert(text, &paths.block()).unwrap();
        assert!(inserted.starts_with(text));
        assert!(dsh_insert("[{insert: []}]", &paths.block()).is_err());
    }
    #[test]
    #[cfg(not(feature = "production"))]
    fn development_never_reads_or_writes_real_client_configuration() {
        for client in [
            Client::Codex,
            Client::ClaudeCode,
            Client::DeepseekHarness,
            Client::Antigravity,
        ] {
            let status = ai_integration_status(
                client,
                Some(Locations {
                    config_path: Some("bad path".into()),
                    skill_path: None,
                }),
            )
            .unwrap();
            assert_eq!(status.state, "development");
            assert!(!status.can_configure);
            assert!(configure_ai_integration(client, None)
                .unwrap_err()
                .contains("开发版"));
            assert!(remove_ai_integration(client, None)
                .unwrap_err()
                .contains("开发版"));
        }
    }
}
