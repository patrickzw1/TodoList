# Local AI client integration

TodoList supports **Codex**, **Claude Code (CC)**, **DeepSeek Harness (DSH)** and **Google Antigravity** through the bundled native STDIO sidecar. Each client starts its own process. Production processes share the installed app's channel database. Development builds use their own database. MCP can work with the task board closed.

**DSH read-only client acceptance:** the user confirmed a real `list_projects` call through DeepSeek Harness desktop `0.2.0-rc.2` and then removed the temporary integration. This proves that one read-only tool was discovered and invoked in that setup; the other tools and write operations have not been verified there. The optional acceptance script still does not exercise Harness. Desktop installations can contain an embedded runtime even when `dsh` is absent from PATH.

## Client locations and discovery

| Client | User MCP registration | Personal Skill directory |
| --- | --- | --- |
| Codex | `$CODEX_HOME/config.toml`, default `~/.codex/config.toml` | `~/.agents/skills/todolist-mcp` |
| CC | `~/.claude.json`; when set, `$CLAUDE_CONFIG_DIR/.claude.json` | `$CLAUDE_CONFIG_DIR/skills/todolist-mcp`, default `~/.claude/skills/todolist-mcp` |
| DSH | `$DSH_HOME/cordis.patch.yml`, default `~/.dsh/cordis.patch.yml` | `$DSH_HOME/skills/todolist-mcp` |
| Google Antigravity | `~/.gemini/config/mcp_config.json` | `~/.gemini/config/skills/todolist-mcp` |

Antigravity App and CLI share the global Skill directory. If discovery fails in the App, use **Copy Config File Path** in its MCP settings to find the path to check in TodoList. After syncing the integration, restart the `todolist` server or reload the client and run the read-only `list_projects` tool. [Antigravity MCP](https://antigravity.google/docs/mcp) · [Antigravity Skills](https://antigravity.google/docs/skills)

Detection reads executable/configuration evidence. A configuration directory alone does not prove that a runnable client is installed. The integration page reports that distinction. It displays MCP registration and Skill state separately. **Configured means synchronized files on disk**, never a live connection or granted tool permission.

Select one client, inspect the exact paths, and confirm its configure, update, migration or remove action. Other clients are untouched. Manual absolute config and Skill paths are available for nonstandard installations. Check those paths before confirming; the chosen paths persist only after successful configuration. Select paths that the client actually loads; project settings, Skill precedence and managed restrictions can override user settings.

Switching clients preserves each client's manual draft, checked paths and errors together. Confirmation is cancelled on a switch or edit. Configure and remove always send the exact paths displayed after a successful check, including automatically discovered paths. A failed refresh invalidates that check and disables both actions until the selected paths are successfully read again.

The shared Skill retains version rereads, stable request IDs, conflict handling, compact task output, attachment import and explicit paginated activity reads. CC, DSH and Antigravity add client-specific discovery and permission guidance. Existing client tool approvals apply.

MCP task/file metadata retains runtime-only localPath/localPathStatus for managed copies on the MCP host. Compact lists do not expand files. The [MCP API](MCP_API.md) provides17 consolidated tools for search/summary, selected detail reads, versioned local/atomic batch edits, file submission/removal and on-demand text/PDF/DOCX/PPTX/XLSX/image reads. Native parsing runs without Python, Office or cloud services. Ordinary reads never return bodies; image/scan OCR and unsupported/encrypted/corrupt/partial states are explicit. Actual image delivery/rendering must be verified separately from registration/tool discovery. Documents remain untrusted data.

Known owned complete legacy tool sets are offered an explicit upgrade; custom client permission restrictions are never widened. CC/DSH adapters preserve their native approval controls and add no allowed-tools grants. See delivery acceptance evidence for separately verified CLI versions/protocols; no static UI warning claims unverified runtime health.

## Ownership, backups and failures

Codex retains its existing managed-marker/command migration rules and complete legacy tool-list upgrades. Custom restricted tool lists are preserved. Empty old arguments can be explicitly updated to add the client origin; unknown arguments are a conflict.

CC uses the user JSON `mcpServers.todolist` entry. Its receipt verifies the generated type, command and arguments. Additional entry fields, other servers, preferences and permissions are preserved. DSH appends a marked patch `insert` for `@deepseek-ai/dsh-mcp-client`, with `serverName: todolist`, `transport: stdio` and the native command. The managed block must match its receipt; every byte outside it is preserved, including comments, YAML tags and aliases. Unowned same-name entries/patches, modified managed launch fields, malformed data, linked paths and incompatible YAML layouts are left unchanged.

Antigravity also uses `mcpServers.todolist` in its user JSON. Its STDIO entry has the absolute installed sidecar `command` and empty `args`. The receipt binds the launch command to the chosen client and path. A remote `serverUrl`, changed arguments or an unowned same-name entry is a conflict. In-place reconciliation preserves server options including `disabled`, `enabledTools`, `disabledTools` and `tools`.

Updates replace the selected client's current `SKILL.md` in place. They do not install another discoverable copy. Before changing owned files, TodoList saves the owned Skill files in a single sibling `<skill-name>.todolist-recovery-<UUID>.json` recovery file. It contains no filesystem entry named `SKILL.md`, so recursive Skill discovery cannot load it; safety does not depend on a hidden directory name. Full client-config backups remain beside their own configuration file, outside the Skill recovery envelope. Already synchronized configuration with unchanged saved paths creates no backup and performs no rewrite.

An explicit configure, update or remove also retires the older sibling `<skill-name>.todolist-backup-<UUID>` directories that TodoList previously generated. It accepts only exact canonical version4 UUID names, the selected client's exact ownership marker, and the known small set of ordinary Skill/metadata files. Symbolic links, Windows junctions/reparse points, unknown content and conflicting recovery files are preserved with a message. Other Skill names and other clients are not traversed or removed. The validated old files are first saved durably to a non-Skill JSON envelope, checked again, then removed individually; no recursive deletion is used. Read-only status checks do not retire backups.

File-operation failures roll back the selected client's configuration and Skill files and retain recovery data; rollback failures are reported. A blocked legacy retirement happens after the integration operation succeeds and is reported separately, without claiming that the successful configuration failed. An identical retry can finish retirement after an occupied file is released. Remove deletes only recognized MCP/Skill files; unknown files remain. Unrecognized remnants require manual resolution before reinstalling into the same directory.

Recovery envelopes use `format: "todolist-skill-recovery"`, `version: 1`, `client`, and a `files` map from owned basename to its original UTF-8 content, including the prior Skill and available ownership metadata. They deliberately exclude the full client configuration, which can contain private settings. Keep recovery files private and do not publish real client backups. Manual recovery requires checking the chosen client/path and the current configuration first; extracting an old `SKILL.md` into another Skill directory would recreate the duplicate this update prevents. This change does not automatically inspect or clean any real local backup in development tests.

DSH's home patch is applied after profile bundles and the profile patch, but later explicit patch layers can override it. The active profile must resolve **`@deepseek-ai/dsh-mcp-client`**. TodoList does not install Harness packages or claim compatibility from raw JSON alone. The current official client probes STDIO protocol versions before opening its serving process; verify tool discovery in the actual Harness profile after restarting.

## Origin and multiple processes

Codex, CC and DSH pass their documented `--client` value. Antigravity starts the sidecar with zero arguments and uses the existing generic AI actor and retry scope, which also works with the 0.2.23 sidecar. The sidecar determines the actor at startup. Existing `Codex 创建`/`codex` history is readable.

Request IDs are scoped by client so independent clients may use the same ID without replaying each other's operation. Original Codex retry keys remain compatible. SQLite initialization is serialized with a sibling OS file lock (released automatically on exit), including WAL setup and legacy compaction. Task/workspace writes retain their existing atomic version conflicts; after a conflict re-read and preserve newer changes. Sidecar stdout remains protocol-only.

## Verification

- `cargo test --workspace --lib` — isolated configuration lifecycle, migrations, restricted tools, ownership, backup/rollback and storage/tool regressions.
- `npm run test:app` and `npm run typecheck` — client state aggregation, update badges, switched-client configure/remove path binding, refresh failures and backup origin compatibility.
- `cargo build -p todolist-mcp`, then `npm run test:mcp-multiclient` — simultaneous client processes in one unique temporary database, origin, retry scoping, version conflict, stale cursors, attachments and restart persistence.
- `TODOLIST_MCP_EXECUTABLE` can select the tested sidecar for either smoke script. `npm run test:mcp-stdio` preserves the broader single-client protocol regression.
- `scripts/ai-client-acceptance.mjs` optionally uses absolute `TODOLIST_CODEX_EXECUTABLE` and `TODOLIST_CLAUDE_EXECUTABLE` paths. It isolates homes, configuration, credentials and database, avoids model calls, cleans fixtures and writes sanitized evidence to `target/AI_CLIENT_ACCEPTANCE.json`. Unavailable or incomplete real-client discovery is recorded as a limitation.
- The user separately confirmed real DSH desktop `0.2.0-rc.2` discovery and a read-only `list_projects` invocation, then removed that test integration. This does not validate other DSH tools or writing to the daily workspace.

Official adapter references: [Claude MCP](https://code.claude.com/docs/en/mcp), [Claude Skills](https://code.claude.com/docs/en/skills), [DSH MCP client](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/mcp/mcp-client/README.md), [DSH Skills](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/skills.md), [DSH app boot/layers](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/boot/app-boot/README.md). DSH source was reviewed at `639ed015397290b3745d163aafe02ffee4aa3f84`.
