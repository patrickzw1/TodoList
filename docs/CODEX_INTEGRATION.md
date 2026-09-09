# Codex integration

TodoList remains a standalone local app. The optional `todolist-mcp` STDIO server gives Codex eight tools:

- `list_projects`
- `create_project`
- `list_tasks`
- `get_task`
- `get_task_activity`
- `create_task`
- `update_task`
- `reorder_tasks`

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
enabled_tools = ["list_projects", "create_project", "list_tasks", "get_task", "get_task_activity", "create_task", "update_task", "reorder_tasks"]
default_tools_approval_mode = "writes"
```

On Windows the executable ends in `.exe`. On macOS it has no extension. The project launcher only starts `target/development/release/todolist-mcp`; it never falls back to legacy `target/debug`, `target/release`, or an executable from the environment. The smoke-test script can explicitly select a binary with `TODOLIST_MCP_EXECUTABLE`.

Desktop and MCP share the same storage-path resolver. All ordinary builds, including `cargo build --release`, use the `app.todolist.desktop.dev` application-data directory. The explicit `production` Cargo feature selects the existing `app.todolist.desktop` directory. Release configuration enables that feature for both binaries. The desktop refuses to start if its application identifier does not match the compiled storage channel. The application identifier also separates WebView storage. On Windows, these directories are under `%APPDATA%`.

Tests may explicitly override the database for either binary with `TODOLIST_DB_PATH`; do not set that variable globally. `todolist-mcp --print-storage-path` prints its resolved path and exits without opening or creating a database. Existing user data is never automatically moved, cleared, or copied into the development database.

For an older developer machine whose global `todolist` still points into this repository, back up and remove only that owned registration before configuring the installed app. Keep the existing user database. Once configured, daily tasks use `todolist` even inside this repository; development tests explicitly use `todolist_dev`. Existing MCP processes retain their old binary until Codex restarts.

## Attachments and images

Normal task results from `list_tasks`, `get_task`, `create_task`, and `update_task` omit `activity` while retaining the task version, checklist ids, dependencies, attachments, images, and every other task field. Use the read-only `get_task_activity` tool only when the user asks for history. It returns newest entries first, defaults to 10, accepts 1 through 50, and uses an opaque cursor bound to the task id and current task version. A task change makes an existing history cursor stale, so restart that history read without the cursor. The retained activity data remains stored on the task and bounded to the newest 100 entries.

Task details support separate attachments and image collections. `get_task` and `list_tasks` include their metadata (`id`, `originalName`, `mediaType`, `size`, `storageKey`, `addedAt`); they do not return file content. `update_task` and `reorder_tasks` preserve both collections automatically. Files are added, removed, opened, and previewed in the desktop task-detail panel. The eight MCP tools do not upload, import, remove, or preview files, and putting a path in a description does not attach it.

After upgrading, use the installed app's Codex integration page to confirm and refresh the TodoList-managed integration if it shows an update is needed, then reconnect or restart Codex to reload the bundled MCP server and its capability descriptions. This explicit refresh updates the managed Skill and adds newly managed tools such as `get_task_activity` to a known complete older `todolist` tool list while preserving unrelated settings, user-restricted tool lists, and other MCP servers. The app does not silently rewrite the user's Codex configuration during an upgrade.

## Task safety

- Every task carries a version. `update_task` rejects stale versions instead of silently overwriting user changes.
- `list_tasks` uses versioned opaque cursor pagination with a default page size of 50 and a maximum of 100. Any intervening workspace change makes an old cursor stale, so restart the filtered listing without that cursor.
- `get_task_activity` reads one task's retained history newest-first, defaults to 10 entries and accepts 1 through 50. Its opaque cursor is bound to that task and task version; task updates and history truncation invalidate the cursor instead of silently duplicating or skipping entries.
- `reorder_tasks` requires every stable task id in one project and archived-state group exactly once plus the latest workspace version. To order archived tasks, call `list_tasks` with `include_archived=true`, read every page, and filter the returned items to `archived=true` on the client because `list_tasks` has no archived-only filter. It changes only shared order and never changes task fields or task versions.
- `create_project` and `create_task` accept a stable `request_id`. Retrying the same input returns the original item; reusing that key for different input is rejected. The small retry ledger is capped at 1,000 records.
- After a conflict, Codex must re-read and preserve newer user edits.
- Completed tasks are not reopened unless the user explicitly requests it.
- MCP-created tasks are unpinned and never open the desktop note.
- TodoList does not trigger or resume Codex in this MVP.
