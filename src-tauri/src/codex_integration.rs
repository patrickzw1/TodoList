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
const MCP_TOOLS: [&str; 6] = [
    "list_projects",
    "create_project",
    "list_tasks",
    "get_task",
    "create_task",
    "update_task",
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
    current: bool,
    managed_command: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexIntegrationStatus {
    state: String,
    configured: bool,
    can_configure: bool,
    managed_migration: bool,
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
    let skill_matches = fs::read_to_string(paths.skill_directory.join("SKILL.md"))
        .is_ok_and(|content| content == SKILL_CONTENT);
    Ok(SkillState {
        exists: true,
        managed: marker_matches,
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
    let config_matches = command.is_some_and(|item| item == expected_command);
    let config_exists = configured_server_exists(&document);
    let skill = skill_state(paths)?;
    let managed_migration = command
        .is_some_and(|item| !config_matches && is_managed_previous_command(paths, &skill, item));

    let (state, configured, can_configure, message) = if config_matches && skill.current {
        (
            "configured",
            true,
            true,
            "当前 TodoList 安装已配置 Codex 集成",
        )
    } else if managed_migration {
        (
            "partial",
            false,
            true,
            "检测到由 TodoList 管理的旧路径，可以迁移到当前安装",
        )
    } else if (config_exists && !config_matches) || (skill.exists && !skill.managed) {
        (
            "conflict",
            false,
            false,
            "检测到不属于当前安装的 TodoList 集成，已保持原样",
        )
    } else if config_exists || skill.exists {
        (
            "partial",
            false,
            true,
            "TodoList 集成配置不完整，可以安全修复",
        )
    } else {
        ("not_configured", false, true, "尚未配置 Codex 集成")
    };

    Ok(CodexIntegrationStatus {
        state: state.to_string(),
        configured,
        can_configure,
        managed_migration,
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

fn install_config(paths: &IntegrationPaths) -> Result<(), String> {
    let mut document = read_document(&paths.codex_config)?;
    let expected_command = display_path(&paths.mcp_executable);
    if let Some(command) = configured_command(&document).map(str::to_string) {
        if command == expected_command {
            return Ok(());
        }
        let skill = skill_state(paths)?;
        if !is_managed_previous_command(paths, &skill, &command) {
            return Err("A different TodoList MCP server is already configured".to_string());
        }
        document["mcp_servers"]["todolist"]["command"] = value(expected_command);
        backup_file(&paths.codex_config)?;
        return atomic_write(&paths.codex_config, &document.to_string());
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
    let mut enabled_tools = Array::new();
    for tool in MCP_TOOLS {
        enabled_tools.push(tool);
    }
    server["enabled_tools"] = value(enabled_tools);
    server["default_tools_approval_mode"] = value("writes");
    servers.insert("todolist", Item::Table(server));

    backup_file(&paths.codex_config)?;
    atomic_write(&paths.codex_config, &document.to_string())
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
    if !paths.mcp_executable.is_file() {
        return Err("TodoList MCP executable is missing".to_string());
    }
    let status = inspect(&paths)?;
    if !status.can_configure {
        return Err(status.message);
    }
    install_skill(&paths)?;
    install_config(&paths)?;
    write_managed_command(&paths)?;
    inspect(&paths)
}

#[tauri::command]
pub fn remove_codex_integration() -> Result<CodexIntegrationStatus, String> {
    require_production_integration()?;
    let paths = user_paths()?;
    remove_config(&paths)?;
    remove_skill(&paths)?;
    inspect(&paths)
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
        install_config(&paths).unwrap();
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
        assert!(install_config(&paths).is_err());
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
        install_config(&old_paths).unwrap();
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

        install_config(&new_paths).unwrap();
        write_managed_command(&new_paths).unwrap();
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
        install_config(&old_paths).unwrap();

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
        install_config(&new_paths).unwrap();
        write_managed_command(&new_paths).unwrap();
        assert!(inspect(&new_paths).unwrap().configured);
    }

    #[test]
    fn refuses_a_changed_command_even_when_the_skill_is_managed() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        fs::create_dir_all(paths.codex_config.parent().unwrap()).unwrap();
        install_skill(&paths).unwrap();
        install_config(&paths).unwrap();
        write_managed_command(&paths).unwrap();
        fs::write(
            &paths.codex_config,
            "[mcp_servers.todolist]\ncommand = \"other-server\"\n",
        )
        .unwrap();

        let conflict = inspect(&paths).unwrap();
        assert_eq!(conflict.state, "conflict");
        assert!(!conflict.can_configure);
        assert!(install_config(&paths).is_err());
    }
}
