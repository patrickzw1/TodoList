use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use task_store_sqlite::storage_path::IS_PRODUCTION;
use tempfile::NamedTempFile;
use toml_edit::{value, Array, DocumentMut, Item, Table};

const SKILL_CONTENT: &str = include_str!("../../.agents/skills/todolist-mcp/SKILL.md");
const MANAGED_MARKER: &str = "app.todolist.desktop\n";
const MANAGED_COMMAND_FILE: &str = ".todolist-command";
const LEGACY_MCP_TOOLS_V1: [&str; 6] = [
    "list_projects",
    "create_project",
    "list_tasks",
    "get_task",
    "create_task",
    "update_task",
];
const LEGACY_MCP_TOOLS_V2: [&str; 7] = [
    "list_projects",
    "create_project",
    "list_tasks",
    "get_task",
    "create_task",
    "update_task",
    "reorder_tasks",
];
const MCP_TOOLS: [&str; 8] = [
    "list_projects",
    "create_project",
    "list_tasks",
    "get_task",
    "get_task_activity",
    "create_task",
    "update_task",
    "reorder_tasks",
];

#[derive(Clone)]
struct IntegrationPaths {
    codex_config: PathBuf,
    skill_directory: PathBuf,
    mcp_executable: PathBuf,
}

#[derive(Default)]
struct SkillState {
    exists: bool,
    managed: bool,
    skill_file_exists: bool,
    current: bool,
    managed_command: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum ConfigChange {
    None,
    Created,
    AddedManagedTools(Vec<String>),
    MigratedCommand,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexIntegrationStatus {
    state: String,
    configured: bool,
    can_configure: bool,
    managed_migration: bool,
    reason: String,
    pending_updates: Vec<String>,
    action_result: String,
    updated_items: Vec<String>,
    config_path: String,
    skill_path: String,
    mcp_command: String,
    message: String,
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn resolve_mcp_executable() -> Result<PathBuf, String> {
    let executable_name = if cfg!(windows) {
        "todolist-mcp.exe"
    } else {
        "todolist-mcp"
    };
    let app_executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let installed_candidate = app_executable
        .parent()
        .ok_or_else(|| "TodoList executable has no parent directory".to_string())?
        .join(executable_name);
    if installed_candidate.is_file() {
        return Ok(installed_candidate);
    }

    Err("TodoList MCP executable is not bundled with this build".to_string())
}

fn user_paths() -> Result<IntegrationPaths, String> {
    let home =
        dirs::home_dir().ok_or_else(|| "Could not locate the user home directory".to_string())?;
    Ok(IntegrationPaths {
        codex_config: home.join(".codex").join("config.toml"),
        skill_directory: home.join(".agents").join("skills").join("todolist-mcp"),
        mcp_executable: resolve_mcp_executable()?,
    })
}

fn read_document(path: &Path) -> Result<DocumentMut, String> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Codex config is not a regular file; TodoList will not modify it".to_string());
    }
    fs::read_to_string(path)
        .map_err(|error| error.to_string())?
        .parse::<DocumentMut>()
        .map_err(|error| format!("Codex config is not valid TOML: {error}"))
}

fn configured_command(document: &DocumentMut) -> Option<&str> {
    document
        .as_table()
        .get("mcp_servers")?
        .as_table()?
        .get("todolist")?
        .as_table()?
        .get("command")?
        .as_str()
}

fn configured_server_exists(document: &DocumentMut) -> bool {
    document
        .as_table()
        .get("mcp_servers")
        .and_then(Item::as_table)
        .is_some_and(|servers| servers.contains_key("todolist"))
}

fn configured_tools_item(document: &DocumentMut) -> Option<&Item> {
    document
        .as_table()
        .get("mcp_servers")?
        .as_table()?
        .get("todolist")?
        .as_table()?
        .get("enabled_tools")
}

fn configured_tools_valid(document: &DocumentMut) -> bool {
    let Some(item) = configured_tools_item(document) else {
        return true;
    };
    let Some(tools) = item.as_array() else {
        return false;
    };
    let mut configured_tools = Vec::with_capacity(tools.len());
    for tool in tools.iter() {
        let Some(tool) = tool.as_str() else {
            return false;
        };
        if configured_tools.contains(&tool) {
            return false;
        }
        configured_tools.push(tool);
    }
    true
}

fn configured_tools_match(document: &DocumentMut, expected_tools: &[&str]) -> bool {
    let Some(tools) = configured_tools_item(document).and_then(Item::as_array) else {
        return false;
    };
    tools.len() == expected_tools.len()
        && expected_tools.iter().all(|expected| {
            tools
                .iter()
                .any(|configured| configured.as_str() == Some(expected))
        })
}

fn configured_tools_current(document: &DocumentMut) -> bool {
    configured_tools_match(document, &MCP_TOOLS)
}

fn configured_tools_legacy(document: &DocumentMut) -> bool {
    configured_tools_match(document, &LEGACY_MCP_TOOLS_V1)
        || configured_tools_match(document, &LEGACY_MCP_TOOLS_V2)
}

fn missing_managed_tools(document: &DocumentMut) -> Vec<&'static str> {
    let Some(configured) = configured_tools_item(document).and_then(Item::as_array) else {
        return Vec::new();
    };
    MCP_TOOLS
        .iter()
        .copied()
        .filter(|expected| {
            !configured
                .iter()
                .any(|configured| configured.as_str() == Some(expected))
        })
        .collect()
}

fn tool_update_label(tools: &[&str]) -> String {
    format!("工具列表（新增 {}）", tools.join("、"))
}

fn enabled_tools_value() -> toml_edit::Value {
    let mut enabled_tools = Array::new();
    for tool in MCP_TOOLS {
        enabled_tools.push(tool);
    }
    toml_edit::Value::Array(enabled_tools)
}

fn read_managed_command(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    let command = fs::read_to_string(path).ok()?;
    let command = command.trim();
    (!command.is_empty()).then(|| command.to_string())
}

fn skill_state(paths: &IntegrationPaths) -> Result<SkillState, String> {
    if !paths.skill_directory.exists() {
        return Ok(SkillState::default());
    }
    let metadata =
        fs::symlink_metadata(&paths.skill_directory).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(SkillState {
            exists: true,
            ..SkillState::default()
        });
    }
    let marker_matches = fs::read_to_string(paths.skill_directory.join(".todolist-managed"))
        .is_ok_and(|content| content == MANAGED_MARKER);
    let skill_path = paths.skill_directory.join("SKILL.md");
    let skill_file_exists = skill_path.is_file();
    let skill_matches =
        fs::read_to_string(&skill_path).is_ok_and(|content| content == SKILL_CONTENT);
    Ok(SkillState {
        exists: true,
        managed: marker_matches,
        skill_file_exists,
        current: marker_matches && skill_matches,
        managed_command: marker_matches
            .then(|| read_managed_command(&paths.skill_directory.join(MANAGED_COMMAND_FILE)))
            .flatten(),
    })
}

fn files_have_same_contents(left: &Path, right: &Path) -> bool {
    let Ok(left_metadata) = fs::symlink_metadata(left) else {
        return false;
    };
    let Ok(right_metadata) = fs::symlink_metadata(right) else {
        return false;
    };
    if left_metadata.file_type().is_symlink()
        || right_metadata.file_type().is_symlink()
        || !left_metadata.is_file()
        || !right_metadata.is_file()
        || left_metadata.len() != right_metadata.len()
    {
        return false;
    }
    fs::read(left)
        .and_then(|left_content| fs::read(right).map(|right_content| left_content == right_content))
        .unwrap_or(false)
}

fn is_managed_previous_command(
    paths: &IntegrationPaths,
    skill: &SkillState,
    command: &str,
) -> bool {
    if !skill.managed {
        return false;
    }
    if skill.managed_command.as_deref() == Some(command) {
        return true;
    }

    let configured_path = Path::new(command);
    configured_path.file_name() == paths.mcp_executable.file_name()
        && files_have_same_contents(configured_path, &paths.mcp_executable)
}

fn inspect(paths: &IntegrationPaths) -> Result<CodexIntegrationStatus, String> {
    let document = read_document(&paths.codex_config)?;
    let command = configured_command(&document);
    let expected_command = display_path(&paths.mcp_executable);
    let command_matches = command.is_some_and(|item| item == expected_command);
    let config_exists = configured_server_exists(&document);
    let skill = skill_state(paths)?;
    let managed_migration = command
        .is_some_and(|item| !command_matches && is_managed_previous_command(paths, &skill, item));
    let current_tools = command_matches
        && (configured_tools_item(&document).is_none() || configured_tools_current(&document));
    let managed_legacy_tools = command_matches
        && skill.managed
        && skill.skill_file_exists
        && !skill.current
        && configured_tools_legacy(&document);
    let missing_tools = if managed_legacy_tools {
        missing_managed_tools(&document)
    } else {
        Vec::new()
    };
    let skill_update_available =
        command_matches && skill.managed && skill.skill_file_exists && !skill.current;

    let (state, configured, can_configure, reason, message, pending_updates) = if managed_migration
    {
        (
                "partial",
                false,
                true,
                "path_migration",
                "检测到 TodoList 管理的旧安装路径，需要迁移 MCP 命令；工具权限与其他 Codex 配置将保持不变。",
                Vec::new(),
            )
    } else if config_exists && !command_matches {
        (
            "conflict",
            false,
            false,
            "unknown_config",
            "检测到未知或不属于当前安装的 TodoList MCP 配置，已保持原样。",
            Vec::new(),
        )
    } else if skill.exists && !skill.managed {
        (
            "conflict",
            false,
            false,
            "unmanaged_skill",
            "检测到不属于 TodoList 管理的同名 Skill，已保持原样。",
            Vec::new(),
        )
    } else if command_matches && !configured_tools_valid(&document) {
        (
            "partial",
            false,
            false,
            "invalid_tools",
            "TodoList enabled_tools 配置无效：该字段必须是无重复字符串数组；已保持原样。",
            Vec::new(),
        )
    } else if command_matches && (!skill.exists || !skill.skill_file_exists) {
        (
            "partial",
            false,
            true,
            "missing_skill",
            "缺少 TodoList 管理的 Skill 文件，需要重新配置；现有 MCP 工具权限将保持不变。",
            Vec::new(),
        )
    } else if skill_update_available || managed_legacy_tools {
        let mut updates = Vec::new();
        if skill_update_available {
            updates.push("新版使用说明".to_string());
        }
        if managed_legacy_tools {
            updates.push(tool_update_label(&missing_tools));
        }
        let message = if managed_legacy_tools {
            "安装路径未变，需要同步新版使用说明和工具列表。"
        } else {
            "安装路径未变，需要同步新版使用说明。"
        };
        (
            "update_available",
            false,
            true,
            "managed_update",
            message,
            updates,
        )
    } else if command_matches && skill.current {
        if current_tools {
            (
                "configured",
                true,
                true,
                "up_to_date",
                "TodoList 的磁盘配置已同步；这不代表 Codex 当前连接状态。",
                Vec::new(),
            )
        } else {
            (
                "configured",
                true,
                true,
                "custom_tools",
                "TodoList 的磁盘配置已同步，并保留了自定义工具权限；这不代表 Codex 当前连接状态。",
                Vec::new(),
            )
        }
    } else if skill.exists && !config_exists {
        (
            "partial",
            false,
            true,
            "missing_config",
            "缺少 TodoList MCP 注册，需要重新配置；其他 Codex 配置将保持不变。",
            Vec::new(),
        )
    } else {
        (
            "not_configured",
            false,
            true,
            "not_configured",
            "尚未配置 Codex 集成。",
            Vec::new(),
        )
    };

    Ok(CodexIntegrationStatus {
        state: state.to_string(),
        configured,
        can_configure,
        managed_migration,
        reason: reason.to_string(),
        pending_updates,
        action_result: String::new(),
        updated_items: Vec::new(),
        config_path: display_path(&paths.codex_config),
        skill_path: display_path(&paths.skill_directory),
        mcp_command: expected_command,
        message: message.to_string(),
    })
}

fn backup_file(path: &Path) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs();
    let backup = path.with_file_name(format!("config.toml.todolist-backup-{timestamp}"));
    fs::copy(path, &backup).map_err(|error| error.to_string())?;
    Ok(Some(backup))
}

fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Target file has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    if path.exists() {
        let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("Target is not a regular file; TodoList will not replace it".to_string());
        }
    }
    let mut temporary = NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    temporary
        .write_all(content.as_bytes())
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| error.to_string())?;
    temporary
        .persist(path)
        .map_err(|error| error.error.to_string())?;
    Ok(())
}

fn install_skill(paths: &IntegrationPaths) -> Result<(), String> {
    let state = skill_state(paths)?;
    if state.exists && !state.managed {
        return Err(
            "The TodoList Skill directory already exists and is not managed by this app"
                .to_string(),
        );
    }
    fs::create_dir_all(&paths.skill_directory).map_err(|error| error.to_string())?;
    atomic_write(&paths.skill_directory.join("SKILL.md"), SKILL_CONTENT)?;
    atomic_write(
        &paths.skill_directory.join(".todolist-managed"),
        MANAGED_MARKER,
    )
}

fn install_config(
    paths: &IntegrationPaths,
    update_managed_legacy_tools: bool,
) -> Result<ConfigChange, String> {
    let mut document = read_document(&paths.codex_config)?;
    let expected_command = display_path(&paths.mcp_executable);
    if let Some(command) = configured_command(&document).map(str::to_string) {
        if command == expected_command {
            if !update_managed_legacy_tools || !configured_tools_legacy(&document) {
                return Ok(ConfigChange::None);
            }
            let added_tools = missing_managed_tools(&document);
            let enabled_tools = document["mcp_servers"]["todolist"]["enabled_tools"]
                .as_array_mut()
                .ok_or_else(|| "TodoList enabled_tools setting is not an array".to_string())?;
            for tool in &added_tools {
                enabled_tools.push(*tool);
            }
            backup_file(&paths.codex_config)?;
            atomic_write(&paths.codex_config, &document.to_string())?;
            return Ok(ConfigChange::AddedManagedTools(
                added_tools.into_iter().map(str::to_string).collect(),
            ));
        }
        let skill = skill_state(paths)?;
        if !is_managed_previous_command(paths, &skill, &command) {
            return Err("A different TodoList MCP server is already configured".to_string());
        }
        document["mcp_servers"]["todolist"]["command"] = value(expected_command);
        backup_file(&paths.codex_config)?;
        atomic_write(&paths.codex_config, &document.to_string())?;
        return Ok(ConfigChange::MigratedCommand);
    }
    if configured_server_exists(&document) {
        return Err("An unrecognized TodoList MCP configuration already exists".to_string());
    }

    if !document.as_table().contains_key("mcp_servers") {
        document["mcp_servers"] = Item::Table(Table::new());
    }
    let servers = document["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| "Codex mcp_servers setting is not a table".to_string())?;
    let mut server = Table::new();
    server["command"] = value(display_path(&paths.mcp_executable));
    server["startup_timeout_sec"] = value(10);
    server["tool_timeout_sec"] = value(15);
    server["required"] = value(false);
    server["enabled_tools"] = value(enabled_tools_value());
    server["default_tools_approval_mode"] = value("writes");
    servers.insert("todolist", Item::Table(server));

    backup_file(&paths.codex_config)?;
    atomic_write(&paths.codex_config, &document.to_string())?;
    Ok(ConfigChange::Created)
}

fn write_managed_command(paths: &IntegrationPaths) -> Result<(), String> {
    atomic_write(
        &paths.skill_directory.join(MANAGED_COMMAND_FILE),
        &format!("{}\n", display_path(&paths.mcp_executable)),
    )
}

fn remove_skill(paths: &IntegrationPaths) -> Result<(), String> {
    let state = skill_state(paths)?;
    if !state.exists {
        return Ok(());
    }
    if !state.managed {
        return Err(
            "The TodoList Skill directory is not managed by this app and was left unchanged"
                .to_string(),
        );
    }
    for name in ["SKILL.md", ".todolist-managed", MANAGED_COMMAND_FILE] {
        let path = paths.skill_directory.join(name);
        if path.exists() {
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
    }
    let _ = fs::remove_dir(&paths.skill_directory);
    Ok(())
}

fn remove_config(paths: &IntegrationPaths) -> Result<(), String> {
    let mut document = read_document(&paths.codex_config)?;
    let Some(command) = configured_command(&document).map(str::to_string) else {
        if configured_server_exists(&document) {
            return Err("The TodoList MCP configuration is not recognized".to_string());
        }
        return Ok(());
    };
    let skill = skill_state(paths)?;
    if command != display_path(&paths.mcp_executable)
        && !is_managed_previous_command(paths, &skill, &command)
    {
        return Err(
            "The configured TodoList MCP command is not managed by this installation".to_string(),
        );
    }

    let servers = document["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| "Codex mcp_servers setting is not a table".to_string())?;
    servers.remove("todolist");
    if servers.is_empty() {
        document.as_table_mut().remove("mcp_servers");
    }
    backup_file(&paths.codex_config)?;
    atomic_write(&paths.codex_config, &document.to_string())
}

#[tauri::command]
pub fn codex_integration_status() -> Result<CodexIntegrationStatus, String> {
    if !IS_PRODUCTION {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        return Ok(CodexIntegrationStatus {
            state: "development".into(),
            configured: false,
            can_configure: false,
            managed_migration: false,
            reason: "development".into(),
            pending_updates: Vec::new(),
            action_result: String::new(),
            updated_items: Vec::new(),
            config_path: display_path(&root.join(".codex/config.toml")),
            skill_path: display_path(&root.join(".agents/skills/todolist-mcp")),
            mcp_command: display_path(&root.join("scripts/start-mcp.mjs")),
            message: "开发版使用项目内的 todolist_dev 和独立开发库；日常集成请在安装版中配置"
                .into(),
        });
    }
    inspect(&user_paths()?)
}

fn require_production_integration() -> Result<(), String> {
    if !IS_PRODUCTION {
        return Err("开发版不能更改全局 Codex 集成，请在安装版中配置 todolist".into());
    }
    Ok(())
}

#[tauri::command]
pub fn configure_codex_integration() -> Result<CodexIntegrationStatus, String> {
    require_production_integration()?;
    let paths = user_paths()?;
    configure(&paths)
}

fn configure(paths: &IntegrationPaths) -> Result<CodexIntegrationStatus, String> {
    if !paths.mcp_executable.is_file() {
        return Err("TodoList MCP executable is missing".to_string());
    }
    let before = inspect(paths)?;
    if !before.can_configure {
        return Err(before.message);
    }

    let mut updated_items = Vec::new();
    let action_result = if before.managed_migration {
        match install_config(paths, false).map_err(|error| format!("迁移 MCP 路径失败：{error}"))?
        {
            ConfigChange::MigratedCommand => updated_items.push("MCP 安装路径".to_string()),
            _ => {
                return Err("TodoList managed path migration did not change the MCP command".into())
            }
        }
        write_managed_command(paths)
            .map_err(|error| format!("记录 TodoList 托管路径失败：{error}"))?;
        "migrated"
    } else {
        let skill_before = skill_state(paths)?;
        let update_managed_legacy_tools = before
            .pending_updates
            .iter()
            .any(|item| item.starts_with("工具列表"));
        let config_change = install_config(paths, update_managed_legacy_tools)
            .map_err(|error| format!("写入 Codex MCP 配置失败：{error}"))?;
        install_skill(paths).map_err(|error| format!("写入 TodoList Skill 失败：{error}"))?;

        if !skill_before.exists || !skill_before.skill_file_exists {
            updated_items.push("TodoList Skill".to_string());
        } else if !skill_before.current {
            updated_items.push("新版使用说明".to_string());
        }
        match config_change {
            ConfigChange::Created => updated_items.push("MCP 注册".to_string()),
            ConfigChange::AddedManagedTools(tools) => {
                let tools = tools.iter().map(String::as_str).collect::<Vec<_>>();
                updated_items.push(tool_update_label(&tools))
            }
            ConfigChange::MigratedCommand => {
                return Err("TodoList MCP path changed while configuring the integration".into())
            }
            ConfigChange::None => {}
        }
        write_managed_command(paths)
            .map_err(|error| format!("记录 TodoList 托管路径失败：{error}"))?;
        if before.state == "update_available" {
            "updated"
        } else {
            "configured"
        }
    };

    let mut after = inspect(paths)?;
    after.action_result = action_result.to_string();
    after.updated_items = updated_items;
    Ok(after)
}

#[tauri::command]
pub fn remove_codex_integration() -> Result<CodexIntegrationStatus, String> {
    require_production_integration()?;
    let paths = user_paths()?;
    remove_config(&paths)?;
    remove_skill(&paths)?;
    let mut status = inspect(&paths)?;
    status.action_result = "removed".into();
    status.updated_items = vec!["TodoList MCP 注册与托管 Skill".into()];
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(feature = "production"))]
    fn development_commands_cannot_modify_global_integration() {
        let status = codex_integration_status().unwrap();
        assert_eq!(status.state, "development");
        assert!(!status.can_configure);
        assert!(configure_codex_integration()
            .unwrap_err()
            .contains("开发版不能"));
        assert!(remove_codex_integration()
            .unwrap_err()
            .contains("开发版不能"));
    }

    fn test_paths(root: &Path) -> IntegrationPaths {
        let executable = root.join(if cfg!(windows) {
            "todolist-mcp.exe"
        } else {
            "todolist-mcp"
        });
        fs::write(&executable, b"test").unwrap();
        IntegrationPaths {
            codex_config: root.join(".codex").join("config.toml"),
            skill_directory: root.join(".agents").join("skills").join("todolist-mcp"),
            mcp_executable: executable,
        }
    }

    #[test]
    fn configures_and_removes_only_todolist_entries() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        fs::create_dir_all(paths.codex_config.parent().unwrap()).unwrap();
        fs::write(
            &paths.codex_config,
            "theme = \"dark\"\n[mcp_servers.existing]\ncommand = \"existing\"\n",
        )
        .unwrap();

        install_skill(&paths).unwrap();
        install_config(&paths, false).unwrap();
        write_managed_command(&paths).unwrap();
        let configured = fs::read_to_string(&paths.codex_config).unwrap();
        assert!(configured.contains("theme = \"dark\""));
        assert!(configured.contains("[mcp_servers.existing]"));
        assert!(configured.contains("[mcp_servers.todolist]"));
        assert!(configured.contains("\"create_project\""));
        assert!(inspect(&paths).unwrap().configured);

        remove_config(&paths).unwrap();
        remove_skill(&paths).unwrap();
        let removed = fs::read_to_string(&paths.codex_config).unwrap();
        assert!(removed.contains("theme = \"dark\""));
        assert!(removed.contains("[mcp_servers.existing]"));
        assert!(!removed.contains("[mcp_servers.todolist]"));
        assert!(!paths.skill_directory.exists());
    }

    #[test]
    fn refreshes_an_owned_six_tool_config_to_all_current_tools_without_replacing_other_settings() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        fs::create_dir_all(paths.codex_config.parent().unwrap()).unwrap();
        install_skill(&paths).unwrap();
        fs::write(
            paths.skill_directory.join("SKILL.md"),
            "old managed version",
        )
        .unwrap();
        write_managed_command(&paths).unwrap();
        fs::write(
            &paths.codex_config,
            format!(
                "theme = \"dark\"\n[mcp_servers.existing]\ncommand = \"existing\"\n[mcp_servers.todolist]\ncommand = {:?}\nenabled_tools = [\"list_projects\", \"create_project\", \"list_tasks\", \"get_task\", \"create_task\", \"update_task\"]\ncustom_setting = \"keep\"\n",
                display_path(&paths.mcp_executable)
            ),
        )
        .unwrap();

        let outdated = inspect(&paths).unwrap();
        assert_eq!(outdated.state, "update_available");
        assert_eq!(outdated.reason, "managed_update");
        assert!(outdated.message.contains("安装路径未变"));
        assert_eq!(outdated.pending_updates.len(), 2);
        assert!(outdated.can_configure);

        let updated = configure(&paths).unwrap();
        assert_eq!(updated.action_result, "updated");
        assert_eq!(
            updated.updated_items,
            vec![
                "新版使用说明",
                "工具列表（新增 get_task_activity、reorder_tasks）"
            ]
        );
        let configured = fs::read_to_string(&paths.codex_config).unwrap();
        assert!(configured.contains("theme = \"dark\""));
        assert!(configured.contains("[mcp_servers.existing]"));
        assert!(configured.contains("custom_setting = \"keep\""));
        assert!(configured.contains("\"reorder_tasks\""));
        assert!(configured.contains("\"get_task_activity\""));
        assert!(configured_tools_current(
            &configured.parse::<DocumentMut>().unwrap()
        ));
        assert!(inspect(&paths).unwrap().configured);
    }

    #[test]
    fn refreshes_an_owned_seven_tool_config_with_only_task_activity() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        fs::create_dir_all(paths.codex_config.parent().unwrap()).unwrap();
        install_skill(&paths).unwrap();
        fs::write(
            paths.skill_directory.join("SKILL.md"),
            "old managed version",
        )
        .unwrap();
        write_managed_command(&paths).unwrap();
        fs::write(
            &paths.codex_config,
            format!(
                "[mcp_servers.todolist]\ncommand = {:?}\nenabled_tools = [\"reorder_tasks\", \"update_task\", \"create_task\", \"get_task\", \"list_tasks\", \"create_project\", \"list_projects\"]\ncustom_setting = \"keep\"\n",
                display_path(&paths.mcp_executable)
            ),
        )
        .unwrap();

        let outdated = inspect(&paths).unwrap();
        assert_eq!(outdated.state, "update_available");
        assert_eq!(
            outdated.pending_updates,
            vec!["新版使用说明", "工具列表（新增 get_task_activity）"]
        );

        let updated = configure(&paths).unwrap();
        assert_eq!(updated.action_result, "updated");
        assert_eq!(
            updated.updated_items,
            vec!["新版使用说明", "工具列表（新增 get_task_activity）"]
        );
        let configured = fs::read_to_string(&paths.codex_config).unwrap();
        assert!(configured.contains("custom_setting = \"keep\""));
        assert_eq!(configured.matches("get_task_activity").count(), 1);
        assert!(configured_tools_current(
            &configured.parse::<DocumentMut>().unwrap()
        ));
    }

    #[test]
    fn treats_a_reordered_complete_tool_list_as_current() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        fs::create_dir_all(paths.codex_config.parent().unwrap()).unwrap();
        install_skill(&paths).unwrap();
        write_managed_command(&paths).unwrap();
        fs::write(
            &paths.codex_config,
            format!(
                "[mcp_servers.todolist]\ncommand = {:?}\nenabled_tools = [\"reorder_tasks\", \"get_task_activity\", \"update_task\", \"create_task\", \"get_task\", \"list_tasks\", \"create_project\", \"list_projects\"]\n",
                display_path(&paths.mcp_executable)
            ),
        )
        .unwrap();

        let status = inspect(&paths).unwrap();
        assert_eq!(status.state, "configured");
        assert_eq!(status.reason, "up_to_date");
        assert!(configured_tools_current(
            &read_document(&paths.codex_config).unwrap()
        ));
    }

    #[test]
    fn preserves_a_user_restricted_tool_list() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        fs::create_dir_all(paths.codex_config.parent().unwrap()).unwrap();
        install_skill(&paths).unwrap();
        write_managed_command(&paths).unwrap();
        fs::write(
            &paths.codex_config,
            format!(
                "[mcp_servers.todolist]\ncommand = {:?}\nenabled_tools = [\"list_projects\", \"create_project\", \"list_tasks\", \"get_task\", \"create_task\", \"update_task\"]\ncustom_setting = \"keep\"\n",
                display_path(&paths.mcp_executable)
            ),
        )
        .unwrap();
        let before = fs::read_to_string(&paths.codex_config).unwrap();

        let status = inspect(&paths).unwrap();
        assert_eq!(status.state, "configured");
        assert_eq!(status.reason, "custom_tools");
        assert!(status.message.contains("自定义工具权限"));
        assert_eq!(install_config(&paths, false).unwrap(), ConfigChange::None);
        let after = fs::read_to_string(&paths.codex_config).unwrap();
        assert_eq!(after, before);
        assert!(!after.contains("reorder_tasks"));
        assert!(!after.contains("get_task_activity"));
    }

    #[test]
    fn treats_absent_tool_permissions_as_the_default() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        fs::create_dir_all(paths.codex_config.parent().unwrap()).unwrap();
        install_skill(&paths).unwrap();
        write_managed_command(&paths).unwrap();
        fs::write(
            &paths.codex_config,
            format!(
                "[mcp_servers.todolist]\ncommand = {:?}\n",
                display_path(&paths.mcp_executable)
            ),
        )
        .unwrap();

        let status = inspect(&paths).unwrap();
        assert_eq!(status.state, "configured");
        assert_eq!(status.reason, "up_to_date");
    }

    #[test]
    fn rejects_structurally_invalid_tool_permissions_without_writing() {
        for enabled_tools in [
            "\"bad\"",
            "[\"list_projects\", 1]",
            "[\"list_projects\", \"list_projects\"]",
        ] {
            let root = tempfile::tempdir().unwrap();
            let paths = test_paths(root.path());
            fs::create_dir_all(paths.codex_config.parent().unwrap()).unwrap();
            install_skill(&paths).unwrap();
            write_managed_command(&paths).unwrap();
            fs::write(
                &paths.codex_config,
                format!(
                    "[mcp_servers.todolist]\ncommand = {:?}\nenabled_tools = {enabled_tools}\n",
                    display_path(&paths.mcp_executable)
                ),
            )
            .unwrap();
            let before = fs::read_to_string(&paths.codex_config).unwrap();

            let status = inspect(&paths).unwrap();
            assert_eq!(status.state, "partial");
            assert_eq!(status.reason, "invalid_tools");
            assert!(!status.configured);
            assert!(!status.can_configure);
            assert!(configure(&paths).is_err());
            assert_eq!(fs::read_to_string(&paths.codex_config).unwrap(), before);
        }
    }

    #[test]
    fn reports_missing_and_unmanaged_components_separately() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let empty = inspect(&paths).unwrap();
        assert_eq!(empty.state, "not_configured");
        assert_eq!(empty.reason, "not_configured");

        fs::create_dir_all(paths.codex_config.parent().unwrap()).unwrap();
        fs::write(
            &paths.codex_config,
            format!(
                "[mcp_servers.todolist]\ncommand = {:?}\nenabled_tools = [\"list_projects\"]\n",
                display_path(&paths.mcp_executable)
            ),
        )
        .unwrap();
        let missing_skill = inspect(&paths).unwrap();
        assert_eq!(missing_skill.state, "partial");
        assert_eq!(missing_skill.reason, "missing_skill");

        fs::create_dir_all(&paths.skill_directory).unwrap();
        fs::write(
            paths.skill_directory.join(".todolist-managed"),
            MANAGED_MARKER,
        )
        .unwrap();
        let missing_skill_file = inspect(&paths).unwrap();
        assert_eq!(missing_skill_file.state, "partial");
        assert_eq!(missing_skill_file.reason, "missing_skill");
        assert!(!missing_skill_file.message.contains("新版"));

        fs::remove_dir_all(&paths.skill_directory).unwrap();
        fs::remove_file(&paths.codex_config).unwrap();
        install_skill(&paths).unwrap();
        let missing_config = inspect(&paths).unwrap();
        assert_eq!(missing_config.state, "partial");
        assert_eq!(missing_config.reason, "missing_config");
    }

    #[test]
    fn returns_a_write_error_from_an_isolated_invalid_config_parent() {
        let root = tempfile::tempdir().unwrap();
        let mut paths = test_paths(root.path());
        let blocked_parent = root.path().join("blocked-config-parent");
        fs::write(&blocked_parent, "not a directory").unwrap();
        paths.codex_config = blocked_parent.join("config.toml");

        let error = configure(&paths).unwrap_err();
        assert!(error.contains("写入 Codex MCP 配置失败"));
        assert!(!paths.codex_config.exists());
        assert!(!paths.skill_directory.exists());
        assert_eq!(inspect(&paths).unwrap().state, "not_configured");
    }

    #[test]
    fn refuses_unmanaged_skill_and_existing_mcp_configuration() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        fs::create_dir_all(&paths.skill_directory).unwrap();
        fs::write(paths.skill_directory.join("SKILL.md"), "user content").unwrap();
        assert!(install_skill(&paths).is_err());

        fs::create_dir_all(paths.codex_config.parent().unwrap()).unwrap();
        fs::write(
            &paths.codex_config,
            "[mcp_servers.todolist]\ncommand = \"other-server\"\n",
        )
        .unwrap();
        assert!(install_config(&paths, false).is_err());
        assert_eq!(
            fs::read_to_string(paths.skill_directory.join("SKILL.md")).unwrap(),
            "user content"
        );
    }

    #[test]
    fn preserves_unknown_files_when_removing_managed_skill() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        install_skill(&paths).unwrap();
        fs::write(paths.skill_directory.join("user-note.txt"), "keep").unwrap();

        remove_skill(&paths).unwrap();

        assert_eq!(
            fs::read_to_string(paths.skill_directory.join("user-note.txt")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn upgrades_an_outdated_managed_skill() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        install_skill(&paths).unwrap();
        fs::write(
            paths.skill_directory.join("SKILL.md"),
            "old managed version",
        )
        .unwrap();

        let outdated = skill_state(&paths).unwrap();
        assert!(outdated.managed);
        assert!(!outdated.current);

        install_skill(&paths).unwrap();
        assert!(skill_state(&paths).unwrap().current);
        assert_eq!(
            fs::read_to_string(paths.skill_directory.join("SKILL.md")).unwrap(),
            SKILL_CONTENT
        );
    }

    #[test]
    fn migrates_a_recorded_managed_command_after_the_install_path_changes() {
        let root = tempfile::tempdir().unwrap();
        let old_paths = test_paths(root.path());
        fs::create_dir_all(old_paths.codex_config.parent().unwrap()).unwrap();
        install_skill(&old_paths).unwrap();
        install_config(&old_paths, false).unwrap();
        write_managed_command(&old_paths).unwrap();

        let new_directory = root.path().join("new-install");
        fs::create_dir_all(&new_directory).unwrap();
        let new_paths = IntegrationPaths {
            codex_config: old_paths.codex_config.clone(),
            skill_directory: old_paths.skill_directory.clone(),
            mcp_executable: new_directory.join(old_paths.mcp_executable.file_name().unwrap()),
        };
        fs::write(&new_paths.mcp_executable, b"new version").unwrap();

        let migration = inspect(&new_paths).unwrap();
        assert!(migration.managed_migration);
        assert!(migration.can_configure);

        let before_config = fs::read_to_string(&new_paths.codex_config).unwrap();
        let migrated = configure(&new_paths).unwrap();
        assert_eq!(migrated.action_result, "migrated");
        assert_eq!(migrated.updated_items, vec!["MCP 安装路径"]);
        let after_config = fs::read_to_string(&new_paths.codex_config).unwrap();
        assert!(after_config.contains("enabled_tools"));
        assert_eq!(
            before_config.matches("reorder_tasks").count(),
            after_config.matches("reorder_tasks").count()
        );
        assert_eq!(
            before_config.matches("get_task_activity").count(),
            after_config.matches("get_task_activity").count()
        );
        assert!(inspect(&new_paths).unwrap().configured);
        let expected_command = display_path(&new_paths.mcp_executable);
        let document = read_document(&new_paths.codex_config).unwrap();
        assert_eq!(
            configured_command(&document),
            Some(expected_command.as_str())
        );
    }

    #[test]
    fn recognizes_a_legacy_managed_command_when_the_binary_is_identical() {
        let root = tempfile::tempdir().unwrap();
        let old_paths = test_paths(root.path());
        fs::create_dir_all(old_paths.codex_config.parent().unwrap()).unwrap();
        install_skill(&old_paths).unwrap();
        install_config(&old_paths, false).unwrap();

        let new_directory = root.path().join("new-install");
        fs::create_dir_all(&new_directory).unwrap();
        let new_paths = IntegrationPaths {
            codex_config: old_paths.codex_config.clone(),
            skill_directory: old_paths.skill_directory.clone(),
            mcp_executable: new_directory.join(old_paths.mcp_executable.file_name().unwrap()),
        };
        fs::copy(&old_paths.mcp_executable, &new_paths.mcp_executable).unwrap();

        let migration = inspect(&new_paths).unwrap();
        assert!(migration.managed_migration);
        configure(&new_paths).unwrap();
        assert!(inspect(&new_paths).unwrap().configured);
    }

    #[test]
    fn refuses_a_changed_command_even_when_the_skill_is_managed() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        fs::create_dir_all(paths.codex_config.parent().unwrap()).unwrap();
        install_skill(&paths).unwrap();
        install_config(&paths, false).unwrap();
        write_managed_command(&paths).unwrap();
        fs::write(
            &paths.codex_config,
            "[mcp_servers.todolist]\ncommand = \"other-server\"\n",
        )
        .unwrap();

        let conflict = inspect(&paths).unwrap();
        assert_eq!(conflict.state, "conflict");
        assert_eq!(conflict.reason, "unknown_config");
        assert!(!conflict.can_configure);
        assert!(install_config(&paths, false).is_err());
    }
}
