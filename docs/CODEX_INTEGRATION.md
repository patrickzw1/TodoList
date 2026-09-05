# Codex integration

TodoList remains a standalone local app. The optional `todolist-mcp` STDIO server gives Codex six tools:

- `list_projects`
- `create_project`
- `list_tasks`
- `get_task`
- `create_task`
- `update_task`

The repository's `.codex/config.toml` enables these tools while Codex is working in this project. Its cross-platform launcher is only for development. The packaged desktop app bundles a native MCP sidecar, so end users do not need Node.js or Rust. Restart Codex after changing MCP configuration. Write tools use Codex's `writes` approval mode.

The MCP registration and the Skill have different jobs:

- `.codex/config.toml` tells Codex how to start and connect to the MCP process. During the STDIO handshake the server publishes its tool names, JSON parameter schemas, and descriptions.
- `.agents/skills/todolist-mcp/SKILL.md` tells Codex when and how to use those tools safely. It does not create the MCP connection.
- The repository copies are discovered automatically only while Codex is working in this repository. Another project or another computer does not inherit them automatically.

The packaged app's integration page offers an explicit `Configure Codex integration` action. Before writing, it shows the exact paths and asks for confirmation. It registers the bundled MCP sidecar in the current user's `~/.codex/config.toml` and installs the usage Skill at `~/.agents/skills/todolist-mcp`. It never writes to the Codex application or source directory.

Existing TOML settings and other MCP servers are preserved. TodoList creates a timestamped backup before changing an existing config, refuses to replace a different `todolist` registration or an unowned Skill directory, and removes only entries and files carrying its ownership marker. A newer TodoList release may update a Skill previously installed by TodoList.

For development or manual configuration, build the server once and point a project's `.codex/config.toml` at the absolute executable path:

```powershell
cargo build --release -p todolist-mcp
```

```toml
[mcp_servers.todolist]
command = "/absolute/path/to/todolist-mcp"
startup_timeout_sec = 10
tool_timeout_sec = 15
required = false
enabled_tools = ["list_projects", "create_project", "list_tasks", "get_task", "create_task", "update_task"]
default_tools_approval_mode = "writes"
```

On Windows the executable ends in `.exe`. On macOS it has no extension. By default the server reads the same application-data SQLite database as the desktop app. Tests may override it with `TODOLIST_DB_PATH`.

## Safety behavior

- Every task carries a version. `update_task` rejects stale versions instead of silently overwriting user changes.
- `list_tasks` uses cursor pagination with a default page size of 50 and a maximum of 100.
- `create_project` and `create_task` accept a stable `request_id`. Retrying the same input returns the original item; reusing that key for different input is rejected. The small retry ledger is capped at 1,000 records.
- After a conflict, Codex must re-read and preserve newer user edits.
- Completed tasks are not reopened unless the user explicitly requests it.
- MCP-created tasks are unpinned and never open the desktop note.
- TodoList does not trigger or resume Codex in this MVP.
