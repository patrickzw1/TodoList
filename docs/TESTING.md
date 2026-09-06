# MVP verification and remaining work

## v0.2.1 local release candidate — 2026-09-07

- `scripts/build-release.ps1` produced the Windows x64 NSIS installer, its Tauri updater signature, `latest.json` and `SHA256SUMS.txt` from the unified `0.2.1` package, Tauri and Rust versions. The candidate has not been committed, pushed, tagged, installed or uploaded.
- The final release GUI reports `0.2.1`; the final MCP reports `todolist/0.2.1/production`. Signature verification against the existing public updater key, manifest URL/version checks, all listed checksums, PE subsystem checks and personal-path scans passed without exposing signing material.
- The dedicated hidden NSIS harness used these final GUI/MCP binaries and the production-aligned `.onInstSuccess` callback. The installer process, `Commit` and cleanup helper recorded the same nonzero PID; the owned cache and helper disappeared after exit. The real failure hook exited `2`, preserved both old files, retained the installer and redirected exactly one Explorer action to the test log.
- Source privacy scanning found no credential, private-key, personal-path, private-chat or personal-email findings. Release binaries likewise contained no scanned personal build path. Public download verification remains intentionally pending until the second-stage GitHub release.

## Windows updater transaction and recovery — 2026-09-07

- Final automated regression passed 47 Rust tests, 43 frontend tests, 4 build-channel/hook checks and 4 Sites checks, plus TypeScript, the production frontend build, a temporary-database MCP STDIO smoke test and both PowerShell parser versions.
- The update UI separates check, download/signature verification and installation states. Network, missing configuration, signature, disk and target-occupancy failures have distinct messages; a download failure never advertises a nonexistent installer.
- The Windows installer replaces Tauri's executable-name-wide process shutdown with exact resolved-path coordination for the target installation directory. The native 64-bit PowerShell path is used from the 32-bit NSIS process so 64-bit GUI/MCP paths can be inspected reliably.
- The GUI and MCP are replaced as one transaction. An installation marker blocks independently restarted MCP instances until both installed component identities pass, then a post-exit cleanup removes only the owned automatic-update attempt.
- Isolated coordinator acceptance covers target GUI/MCP shutdown, a surviving same-named MCP from another directory, the MCP restart race, success cleanup, cancel rollback, failed-installer retention and one-time Explorer state. Fault injection additionally covers interruption before the first backup, between the two component moves, between a move and state persistence, repeated rollback, cleanup-helper launch failure, cache-deletion failure before and after installer removal, later cleanup retry and unexpected cache-content preservation. All Explorer observations use a test-only action log rather than launching a visible window.
- A real custom NSIS installer was compiled with the dedicated `app.todolist.desktop.installer-acceptance` identity. It silently installed into a unique Windows temporary directory containing running target fixtures and an unrelated file, verified matching `0.2.0/production` GUI and MCP components, uninstalled them, removed its unique registry/shortcut entries, preserved the unrelated file and left the other-directory MCP running.
- A later same-version automatic-update probe exposed a modal success-callback failure and was stopped. Its fixture was silently uninstalled and its temporary directory, registry entry and shortcuts were verified removed. The hook now has an explicit no-`MessageBox` regression for silent/passive failure; that UI-bearing full-installer probe was not rerun after the user's report. The dedicated hidden hook harness below replaces only the missing local NSIS lifecycle evidence, without repeating the visible or registered installation path.
- After that stopped probe, a dedicated no-registration NSIS harness compiled byte-for-byte copies of the current `installer-hooks.nsh` and coordinator with matching `0.2.0/production` GUI/MCP payloads. Like the production template, it invokes `NSIS_HOOK_INSTSUCCESS` from NSIS's `.onInstSuccess` callback. The hidden success process exited `0`; its actual process PID matched the nonzero PID recorded by both `Commit` and the cleanup helper, the installed component identities matched, and the owned updater cache plus helper directory disappeared only after installer exit. A second hidden harness injected a real `Prepare` hook failure, exited `2`, restored both original files, retained the installer, recorded `failed`, and redirected exactly one Explorer action to a test log without opening a window. The fixture wrote no registry entries or shortcuts and left no temporary directories.
- This hook-level evidence validates the local NSIS lifecycle and post-exit cleanup, but it is not a signed release artifact and does not cover HTTPS update discovery, download, Tauri signature verification, or the published `latest.json` path. Those release-signing and delivery checks remain separate release acceptance work.
- The acceptance installer did not launch the application, use the daily database, replace the installed TodoList, close Codex, or alter the running development MCP. The fixture build is not an updater-signed release and was not published.

## v0.2.0 release review — 2026-09-06

- 42 Rust tests, 41 frontend tests, 3 build-channel checks, and 4 Sites checks passed, along with TypeScript and the production frontend build. Store and MCP tests were also run with the explicit production feature against temporary databases.
- Additional regressions cover atomic attachment import, stale-save rejection, queued cleanup after restart, re-referenced file preservation, pending restore/import preservation, unsafe storage keys, and incomplete/corrupt backup payloads.
- Browser review confirmed a new origin starts with zero tasks; three-image import and side navigation work; the central image opens the viewer; Tab stays inside the viewer; Escape closes it; two selected tasks can be archived; the archived view has select-all and a counted permanent-delete confirmation. No browser console errors were recorded. The owned preview processes were stopped afterward.
- MCP initialization and tool descriptions document file metadata and desktop-only file management. Existing MCP tools remain unchanged; task updates preserve both file collections. The managed Skill can be upgraded from the installed app's integration page.
- The review did not install over the user's daily app or modify its database. Native file associations and installer upgrade behavior still depend on the target Windows environment. JSON backups retain the documented 25 MB limit.

## Attachments, empty initialization, project deletion and list selection — 2026-09-06

- New desktop workspaces persist `{ projects: [], tasks: [] }`; MCP opens the same empty database without adding examples. Browser examples require the explicit `?demo=1` query. Existing SQLite and browser workspaces are loaded unchanged and legacy tasks normalize missing attachment/image arrays.
- Managed-file tests use temporary roots and verify copy-with-original-preserved, reference-based cleanup, base64 backup payloads, restore under new keys, and unchanged existing files. No daily database or user attachment directory is used.
- Workspace tests cover both project-delete branches, active plus archived task movement, stable-id batch replay, concurrent unarchive protection, filtered selection reconciliation, and attachment/image preservation through MCP updates.
- Browser acceptance covered four-attachment collapse/expand, extension-preserving rows, three-layer image navigation, viewer arrow/Escape behavior, scoped select-all after search, batch archive, batch-delete confirmation, and project task counts. The owned Vite server was stopped and its port verified free; no browser warnings or errors were recorded.

## Development / production isolation — 2026-09-06

- Local desktop builds and the project `todolist_dev` MCP use the independent `app.todolist.desktop.dev` database. Production keeps the existing `app.todolist.desktop` path. The existing user database was preserved and its SHA-256 stayed unchanged during verification.
- 36 Rust tests passed, including separate-store write isolation and rejection of development commands that attempt global integration changes. The 8 store tests also passed with the explicit `production` feature.
- 35 frontend tests, TypeScript, 3 build-channel/launcher checks, the frontend build, and 4 Sites tests passed. The launcher checks reject legacy executable fallback and prevent production sidecars from overwriting the development output directory.
- Both optimized MCP binaries printed their expected default storage paths without opening a database. Each also passed STDIO initialization and six-tool schema smoke checks using its own temporary database.
- Development and explicit production desktop builds both completed with `--no-bundle`; neither was installed or published. The newest development executable is `target/development/release/todolist-desktop.exe`.
- Browser acceptance used a temporary fixture with mocked development IPC: the integration card showed `todolist_dev`, its configuration button was disabled, and no warning/error console entries appeared. This verifies rendering; it does not replace native Windows UI or installer acceptance. The fixture, browser tab, and owned Vite server were removed/stopped afterward.
- The previous owned global `todolist` entry pointing to a legacy development binary was backed up and removed. Other user configuration and the managed Skill were preserved. Register the installed app through its integration page, then restart Codex to replace already-running legacy MCP processes.

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
