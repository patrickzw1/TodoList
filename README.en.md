# TodoList

[简体中文](README.md)

TodoList is a local-first desktop task app. The Windows app works on its own. You can also choose to connect Codex, Claude Code, or DeepSeek Harness to your tasks through a local MCP server.

## Download and use on Windows

Choose a stable version from [GitHub Releases](https://github.com/patrickzw1/TodoList/releases) and download its `TodoList_<version>_x64-setup.exe`. A Windows x64 installer is available; the macOS source is shared, but a macOS installer has not been validated. The installed version is shown in Settings.

A new installation starts with an empty task workspace. Upgrades preserve existing tasks and managed files. Export a JSON backup from Settings regularly. Update packages are verified with a Tauri signature, and installation requires an explicit user action. The Windows installer does not yet have an Authenticode publisher signature, so Windows may show an unknown-publisher prompt. See [Updates](docs/UPDATES.md) for installation and recovery details.

Main features:

- Today, In Progress, All, project, and Archived views; lists, boards, search within the current view, and task ordering.
- Task details, subtasks, acceptance criteria, attachments, images, and activity history; multi-selection, archiving, and confirmed permanent deletion.
- A separate task note that you can pin above other windows; JSON backup and restore; local SQLite storage.
- Version checks and replay of explicit user actions when the desktop UI and MCP change the workspace concurrently.

## AI client integration

In **Connections & Permissions**, select Codex, Claude Code, or DeepSeek Harness, then confirm configuration, update, or removal. Each client's integration is managed separately. Removing one does not remove another client's integration or task data. Integration uses the bundled local STDIO MCP server and a user-level Skill. It does not change client installation directories, grant tool permissions, open the task board, or pin a note automatically. “Configured” describes synchronized files on disk; confirm the live connection in the client. See [AI integration](docs/AI_INTEGRATION.md) for paths and permission boundaries and [Codex integration](docs/CODEX_INTEGRATION.md) for Codex compatibility.

A user successfully invoked the read-only TodoList MCP `list_projects` tool in DeepSeek Harness desktop 0.2.0-rc.2. Other tools and write operations have not been tested in that client.

## Data location

On Windows, a new workspace prefers `%USERPROFILE%\.todolist\app.todolist.desktop\todolist.sqlite`. Managed attachments are stored in `managed-files` beside the database. When an existing legacy `%APPDATA%\app.todolist.desktop\todolist.sqlite` is safe to use, the app continues using it without silently moving or clearing it. Redirected paths or conflicting data require an explicit migration. Development builds use the separate `app.todolist.desktop.dev` channel; the development desktop and MCP share that channel and do not operate on the daily workspace. See [Data backup](docs/DATA_BACKUP.md) for backup and file rules.

## Development and releases

```powershell
npm ci
npm run dev
npm run typecheck
npm run test:app
npm run build
npm run dev:desktop
cargo test --workspace
```

Browser previews start empty; only an explicit `?demo=1` loads demo data. The development MCP is named `todolist_dev`; run `npm run build:sidecar` before connecting it. Ordinary local desktop builds also use the development channel. A production build must explicitly merge `src-tauri/tauri.release.conf.json`. Windows desktop development also requires Rust stable and WebView2.

Pushes to `main` and pull requests run validation only. GitHub Actions builds a production Windows package through the Release workflow. It requires an existing stable `vMAJOR.MINOR.PATCH` tag contained in `main` and matching the source version. Pushing that tag triggers the workflow; you can also manually select the existing tag in the workflow. See [Updates and releases](docs/UPDATES.md) for signing and publication details.
