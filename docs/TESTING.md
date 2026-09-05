# MVP verification and remaining work

## Window lifecycle and local backup — 2026-09-04

Implemented behavior:

- The sticky note opens an existing main window, restoring it first if minimized.
- If the main window was closed, the native command recreates it from `tauri.conf.json`; the sticky note remains open.
- The Open Task Board button blocks duplicate requests while opening and displays a recoverable error on failure.
- Web preview focuses a live opener or opens a named task-board window, reporting popup blocking instead of silently succeeding.
- The sidebar's Desktop Note action creates or reuses the native note and only minimizes the task board after the note has been shown successfully. Row/detail pin actions do not request minimization or focus.
- The sticky header uses a deep native drag region (including its icon and text), excluding the close button. Dragging permission is scoped to the sticky window. The header uses an arrow cursor.
- Windows release builds use the GUI subsystem; debug builds and the STDIO MCP executable retain their console behavior.

Automated checks:

```powershell
cargo test --workspace --jobs 1
npm.cmd run test:windows
npm.cmd run test:app
npm.cmd run typecheck
npm.cmd run build
npm.cmd run test:sites
```

The Rust suite includes window-lifecycle/capability, bounded-activity and native backup safety tests. The JavaScript app suite covers safe empty-project task creation, bounded real-time activity records, backup parsing/version rebasing, and 9 window-call tests using mocked native IPC/browser windows. These tests do not open real OS windows, configure Codex, or modify the user's task database. `test:app` requires Node.js 22 with TypeScript stripping support or newer.

Automated results for this iteration: 26 Rust tests, 18 JavaScript app/window tests, 4 Sites tests, MCP STDIO schema smoke testing, TypeScript checking and frontend production build passed. Windows publisher signing and the online updater release configuration are still absent.

Windows build and native observations:

- Rebuilt the UI and desktop bundles with one Cargo job in the isolated `target/package-build` directory (2026-09-05 00:52 local time). This preserved the active Codex MCP processes that currently lock the development `target/release/todolist-mcp.exe`; no process was terminated.
- Rebuilt the release executable, MSI and `target/release/bundle/nsis/TodoList_0.1.0_x64-setup.exe` with one Cargo job (2026-09-04 17:29 local time). The installers were not installed or published.
- PE headers confirm that `todolist-desktop.exe` now uses subsystem 2 (Windows GUI); `todolist-mcp.exe` remains subsystem 3 (Windows CUI) for STDIO.
- The rebuilt release MCP executable completed a real STDIO initialization and schema check against an isolated temporary database; all six MVP tools were present.
- MCP tests cover project creation, task cursor pagination, persisted retry replay/conflict behavior and the 1,000-record retry-ledger cap.
- Native backup commands passed round-trip, size-limit, schema, symlink/non-regular-file, duplicate-ID and atomic-write tests. The user also verified the real Windows import/export flow in the release application.
- Launched the rebuilt release EXE and used the real native sidebar entry. One sticky window appeared. A state request for the main window explicitly reported that it was minimized.
- Clicking Open Task Board in the sticky note restored the existing main window; its normal native surface could be captured again.
- Native automation dragged the standard task-board titlebar successfully, but its rapid drag gesture did not move the custom sticky header (both text and blank areas). This automated drag check did not pass. After testing the rebuilt note with a physical mouse, the user explicitly confirmed that the note can now be dragged; manual dragging is verified.
- The temporary Vite server was stopped and port 1420 was verified free. The rebuilt native application was left open for the user to continue testing; no task edits were performed during these window checks.

Browser observations:

- Board dragging now uses Pointer Events rather than native HTML drag-and-drop. Preview inspection confirms native `draggable` is absent, cards use the `grab` cursor, and all four columns expose valid drop targets; a physical Windows desktop drag remains the final verification.
- The task-detail scrollbar uses a scoped thin rounded blue-gray thumb in WebView-compatible CSS.
- The All view shows every non-archived task, including undated tasks; automated coverage verifies archived tasks remain excluded.
- Search is visibly labeled and filters only the active task view; automated coverage verifies task fields, tags, acceptance criteria and project names.
- Clicking inside the floating task detail keeps it open. Clicking the transparent outside area closes it through the existing slide-out transition.
- First opening from a standalone sticky page: passed.
- Repeated opening while the task-board page exists: passed, one task-board page.
- Sticky layout remains unchanged; no console errors/warnings were observed.
- After closing the task-board tab through Codex's in-app browser controls, opening it again did not expose another tab. This browser case remains unverified; do not count the native mock test as a real WebView2 or WKWebView round-trip test.
- The new sidebar entry is visible. Direct inspection of the sticky page confirms `cursor: default`, a `deep` header drag region, and `false` on the close button, with no observed console errors or warnings. The in-app browser did not expose a popup from the sidebar click; this web-preview popup case remains unverified.
- The user confirmed that opening the task board from the sticky note works in the previous Windows desktop build.

Remaining native manual checks:

1. Run the built desktop application, pin a task, and confirm the note appears without minimizing the task board. Use the sidebar entry to intentionally minimize it instead.
2. Close only the main task-board window; the note must remain usable.
3. Click Open Task Board in the note. Exactly one main window must open with the latest persisted tasks.
4. Minimize that main window and use the note to restore it.
5. Close and reopen the main window again, then close both windows. Confirm the application exits rather than leaving a hidden process.
6. Dragging is user-verified on Windows. Further coverage can check each header hit area (text, pin icon, blank space) and confirm the close button only closes the note.

## Remaining before public release

- Complete the remaining Windows native checks above. macOS build, installer and window behavior still need a Mac host.
- Set up the GitHub repository/release endpoint and free updater key, with explicit authorization for publication and key handling. Do not advertise functioning online updates before these exist.
- Verify installer upgrade behavior in an isolated directory containing unrelated files; preserve them and the user's app-data database.
- Run a longer-duration resource test. Per-task activity is now capped at the newest 100 entries in the UI, MCP and SQLite persistence paths, but short smoke tests are not proof of stable memory use over months.

Reverse Codex execution, reminders, cloud synchronization and team features remain intentionally outside the MVP.
