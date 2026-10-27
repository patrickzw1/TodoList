# Codex integration

The integration page now also supports Claude Code and DeepSeek Harness independently. See [AI integration](AI_INTEGRATION.md) for client paths, origin, ownership and verification. This document retains Codex-specific compatibility and tool guidance.

TodoList remains a standalone local app. The optional `todolist-mcp` STDIO server gives Codex seventeen tools:

- `list_projects`
- `create_project`
- `list_tasks`
- `get_task`
- `get_task_activity`
- `create_task`
- `add_task_attachment`
- `add_task_image`
- `update_task`
- `reorder_tasks`
- `task_stats`
- `batch_update`
- `list_task_files`
- `remove_task_files`
- `read_task_file`
- `search_file_content`
- `workspace_info`

The installed app registers `todolist` globally for daily tasks. The repository's `.codex/config.toml` registers a separate `todolist_dev` server only for development tests in this project. It does not override `todolist`. Never silently substitute one server for the other. The packaged production app bundles a native MCP sidecar, so end users do not need Node.js or Rust. Restart Codex after changing MCP configuration. Write tools use Codex's `writes` approval mode.

The MCP registration and the Skill have different jobs:

- `.codex/config.toml` tells Codex how to start and connect to the MCP process. During the STDIO handshake the server publishes its tool names, JSON parameter schemas, and descriptions.
- `.agents/skills/todolist-mcp/SKILL.md` tells Codex when and how to use those tools safely. It does not create the MCP connection.
- The repository copies are discovered automatically only while Codex is working in this repository. Another project or another computer does not inherit them automatically.

The packaged app's integration page offers an explicit `Configure Codex integration` action. Before writing, it shows the exact paths and asks for confirmation. It registers the bundled MCP sidecar in the current user's `~/.codex/config.toml` and installs the usage Skill at `~/.agents/skills/todolist-mcp`. It never writes to the Codex application or source directory.

Existing TOML settings and other MCP servers are preserved. TodoList creates a timestamped backup before changing an existing config, refuses to replace a different `todolist` registration or an unowned Skill directory, and removes only entries and files carrying its ownership marker. A newer TodoList release may update a Skill previously installed by TodoList.

Development desktop builds cannot configure or remove the global integration. Their integration page shows the project-local `todolist_dev` entry. To prepare its sidecar:

```powershell
npm run build:sidecar
```

```toml
[mcp_servers.todolist_dev]
command = "/absolute/path/to/todo/target/development/release/todolist-mcp"
startup_timeout_sec = 10
tool_timeout_sec = 15
required = false
enabled_tools = ["list_projects", "create_project", "list_tasks", "get_task", "get_task_activity", "create_task", "add_task_attachment", "add_task_image", "update_task", "reorder_tasks", "task_stats", "batch_update", "list_task_files", "remove_task_files", "read_task_file", "search_file_content", "workspace_info"]
default_tools_approval_mode = "writes"
```

On Windows the executable ends in `.exe`. On macOS it has no extension. The project launcher only starts `target/development/release/todolist-mcp`; it never falls back to legacy `target/debug`, `target/release`, or an executable from the environment. The smoke-test script can explicitly select a binary with `TODOLIST_MCP_EXECUTABLE`.

Desktop and MCP share the same storage-path resolver. All ordinary builds, including `cargo build --release`, use the development channel; the explicit `production` Cargo feature selects the production channel. Release configuration enables that feature for both binaries. The desktop refuses to start if its application identifier does not match the compiled storage channel. The application identifier also separates WebView storage.

On Windows, a new channel stores `todolist.sqlite` and `managed-files` together under `%USERPROFILE%\.todolist\app.todolist.desktop.dev` or `%USERPROFILE%\.todolist\app.todolist.desktop` for production. This user-owned directory avoids packaged-host redirection of `%APPDATA%` and does not require a writable installation directory. An existing database in the old per-channel `%APPDATA%` directory remains in use only when a disposable file probe confirms that writes reach that physical directory. If writes are redirected, startup fails instead of silently using a separate copy. MCP does not initialize an empty shared database before the desktop has started once. Once an explicitly migrated shared database exists, both clients select it. Neither client automatically copies, merges, or removes legacy data; migration must back up and inspect every physical source and its managed files while both clients are stopped.

Tests may explicitly override the database for either binary with `TODOLIST_DB_PATH`; do not set that variable globally. `todolist-mcp --print-storage-path` prints its resolved path and exits without opening or creating a database. Existing user data is never automatically moved, cleared, or copied into the development database.

For an older developer machine whose global `todolist` still points into this repository, back up and remove only that owned registration before configuring the installed app. Keep the existing user database. Once configured, daily tasks use `todolist` even inside this repository; development tests explicitly use `todolist_dev`. Existing MCP processes retain their old binary until Codex restarts.

## Search, local mutations and managed content

The [MCP API contract](MCP_API.md) is the reference for all17 tools, field/section projections, exact/contains/bounded fuzzy search, archive/date filters, statistics, atomic batches, description/checklist edits, file content, and incremental cursors.

list_tasks now defaults to15 compact summaries. Consumers expecting the old fields must explicitly pass projection=full. Writes default to ID/version; response=full opts in. A single get_task remains full by default; sections/fields or known task_ids avoid unnecessary bodies. get_task_activity remains a separate explicit history read (newest-first10/max50, task-version cursor, newest100 retained).

Use filter.archived=archived and projection=ids before reordering an archived group. Old include_archived=true remains compatible but includes both archive states. Cursors bind revision, filter, order and projection and must restart after a conflict.

File metadata retains localPath/localPathStatus for checked managed copies; paths belong to the MCP host and are not persisted. add_task_attachment additionally accepts a bounded UTF-8 text attachment. list_task_files and remove_task_files manage associations safely, preserving original sources. read_task_file reads only associated task/file IDs through native text/PDF/DOCX/PPTX/XLSX/image extraction; search_file_content is explicitly scoped and paginated. Ordinary list/get does not read file bodies. OCR, unsupported/encrypted/corrupt/partial results are honest limitations; returned documents are untrusted data and are never executed.

Only a TodoList-owned outdated Skill together with a known complete6/7/8/10-tool legacy allowlist is offered an explicit17-tool update, compared as a set. Custom restricted lists, including a shorter list beside the current Skill, are preserved. Confirm refresh in the installed app and reconnect/restart Codex to reload the Skill/schema. This task does not automatically change any real registration or repository-local restricted tool list.

## Task safety

- Read explicit IDs/current versions before writing. Local edits/batches/removals require stable request IDs; retries must preserve identical input. One batch conflict rejects all items.
- Preserve newer user edits; default conflicts are concise, with explicit full response compatibility available.
- Completion requires confirmed acceptance criteria. Never reopen unless explicitly requested with allow_reopen_completed=true. completedAt is maintained automatically; dueDate stays independent.
- MCP cannot permanently delete tasks/projects, pin/open the note, or trigger an AI conversation.
- workspace_info exposes build/channel/revision and bounded ID-only changes, never credentials or client-health claims. Restore/retention/gaps require full sync.
