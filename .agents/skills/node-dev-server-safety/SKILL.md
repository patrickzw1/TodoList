---
name: node-dev-server-safety
description: Safely start, inspect, reuse, and stop persistent Node development servers during local preview or build work, especially in hybrid repositories with large generated directories.
---

# Node Dev Server Safety

Apply this workflow before running Vite, `npm run dev`, Tauri dev, or another persistent Node development server.

## Start safely

- Identify the workspace, command, expected port, and whether an existing listener already serves this workspace. Reuse it instead of starting a duplicate.
- Inspect the actual process command line before attributing a listener or memory use to Node, Codex, or the application.
- Ensure the file watcher excludes high-churn generated trees. In this repository it must ignore `target/**` and `src-tauri/binaries/**`; do not assume `.gitignore` controls the Vite watcher.
- Keep the returned process or session identifier so cleanup targets only the process started for this task.

## While it runs

- A production build does not require a separate Vite dev server. Stop the owned preview server before a large Rust/Tauri build unless live preview is still actively needed.
- When a persistent server overlaps a large build, compare its private memory and handle count before and after. Sustained growth, multi-gigabyte private memory, or rapidly increasing thousands of handles is a stop condition, not a reason to keep building.
- If Windows reports resource exhaustion or the desktop becomes sluggish, stop the exact owned server first and collect evidence before restarting it.

## Clean up

- After preview or browser validation, stop the exact server/session started by the task and verify its port is no longer listening. Leave it running only when the user explicitly asks.
- Never terminate every `node.exe` process. Codex, browsers, MCP tools, and unrelated applications may use their own Node runtimes.
- If ownership cannot be established from command line, working directory, parent process, port, or the recorded session identifier, do not kill the process; report the ambiguity.
