# Local AI client integration

TodoList supports **Codex**, **Claude Code (CC)** and **DeepSeek Harness (DSH)** through the same bundled native STDIO sidecar. Each client starts its own process. Production processes share the installed app's existing channel database; development builds use a separate development database. TodoList needs no HTTP service, and the board can be closed while MCP is running.

**DSH read-only client acceptance:** the user confirmed a real `list_projects` call through DeepSeek Harness desktop `0.2.0-rc.2` and then removed the temporary integration. This proves that one read-only tool was discovered and invoked in that setup; the other tools and write operations have not been verified there. The optional acceptance script still does not exercise Harness. Desktop installations can contain an embedded runtime even when `dsh` is absent from PATH.

## Client locations and discovery

| Client | User MCP registration | Personal Skill directory |
| --- | --- | --- |
| Codex | `$CODEX_HOME/config.toml`, default `~/.codex/config.toml` | `~/.agents/skills/todolist-mcp` |
| CC | `~/.claude.json`; when set, `$CLAUDE_CONFIG_DIR/.claude.json` | `$CLAUDE_CONFIG_DIR/skills/todolist-mcp`, default `~/.claude/skills/todolist-mcp` |
| DSH | `$DSH_HOME/cordis.patch.yml`, default `~/.dsh/cordis.patch.yml` | `$DSH_HOME/skills/todolist-mcp` |

Detection reads executable/configuration evidence. A configuration directory alone does not prove that a runnable client is installed. The integration page reports that distinction. It displays MCP registration and Skill state separately. **Configured means synchronized files on disk**, never a live connection or granted tool permission.

Select one client, inspect the exact paths, and confirm its configure, update, migration or remove action. Other clients are untouched. Manual absolute config and Skill paths are available for nonstandard installations. Check those paths before confirming; the chosen paths persist only after successful configuration. Select paths that the client actually loads; project settings, Skill precedence and managed restrictions can override user settings.

Switching clients preserves each client's manual draft, checked paths and errors together. Confirmation is cancelled on a switch or edit. Configure and remove always send the exact paths displayed after a successful check, including automatically discovered paths. A failed refresh invalidates that check and disables both actions until the selected paths are successfully read again.

The shared Skill retains version rereads, stable request IDs, conflict handling, compact task output, attachment import and explicit paginated activity reads. CC and DSH add only their Skill discovery and permission guidance. No `allowed-tools`, permissions allowlist, trust bypass or client approval is added by the Skill.

## Ownership, backups and failures

Codex retains its existing managed-marker/command migration rules and complete legacy tool-list upgrades. Custom restricted tool lists are preserved. Empty old arguments can be explicitly updated to add the client origin; unknown arguments are a conflict.

CC uses the user JSON `mcpServers.todolist` entry. Its receipt verifies the generated type, command and arguments. Additional entry fields, other servers, preferences and permissions are preserved. DSH appends a marked patch `insert` for `@deepseek-ai/dsh-mcp-client`, with `serverName: todolist`, `transport: stdio` and the native command. The managed block must match its receipt; every byte outside it is preserved, including comments, YAML tags and aliases. Unowned same-name entries/patches, modified managed launch fields, malformed data, linked paths and incompatible YAML layouts are left unchanged.

Before changing owned files, TodoList creates unique backups. Skill backups are beside the Skill directory so removing the Skill does not delete them or block reinstall. File-operation failures restore the selected client's original files when possible and report any rollback failure. Remove deletes only recognized MCP/Skill files; unknown files remain. Unrecognized remnants require manual resolution before reinstalling into the same directory.

DSH's home patch is applied after profile bundles and the profile patch, but later explicit patch layers can override it. The active profile must resolve **`@deepseek-ai/dsh-mcp-client`**. TodoList does not install Harness packages or claim compatibility from raw JSON alone. The current official client probes STDIO protocol versions before opening its serving process; verify tool discovery in the actual Harness profile after restarting.

## Origin and multiple processes

Managed commands pass `--client codex`, `--client claude_code` or `--client deepseek_harness`. The sidecar uses that local startup setting for task source and activity actor. Tool arguments, `clientInfo` and model claims cannot change it. Without an argument it records generic AI. Existing `Codex 创建`/`codex` history is preserved.

Request IDs are scoped by client so independent clients may use the same ID without replaying each other's operation. Original Codex retry keys remain compatible. SQLite initialization is serialized with a sibling OS file lock (released automatically on exit), including WAL setup and legacy compaction. Task/workspace writes retain their existing atomic version conflicts; after a conflict re-read and preserve newer changes. Sidecar stdout remains protocol-only.

## Verification

- `cargo test --workspace --lib` — isolated configuration lifecycle, migrations, restricted tools, ownership, backup/rollback and storage/tool regressions.
- `npm run test:app` and `npm run typecheck` — client state aggregation, update badges, switched-client configure/remove path binding, refresh failures and backup origin compatibility.
- `cargo build -p todolist-mcp`, then `npm run test:mcp-multiclient` — simultaneous client processes in one unique temporary database, origin, retry scoping, version conflict, stale cursors, attachments and restart persistence.
- `TODOLIST_MCP_EXECUTABLE` can select the tested sidecar for either smoke script. `npm run test:mcp-stdio` preserves the broader single-client protocol regression.
- `scripts/ai-client-acceptance.mjs` optionally uses absolute `TODOLIST_CODEX_EXECUTABLE` and `TODOLIST_CLAUDE_EXECUTABLE` paths. It isolates homes, configuration, credentials and database, avoids model calls, cleans fixtures and writes sanitized evidence to `target/AI_CLIENT_ACCEPTANCE.json`. Unavailable or incomplete real-client discovery is recorded as a limitation.
- The user separately confirmed real DSH desktop `0.2.0-rc.2` discovery and a read-only `list_projects` invocation, then removed that test integration. This does not validate other DSH tools or writing to the daily workspace.

Official adapter references: [Claude MCP](https://code.claude.com/docs/en/mcp), [Claude Skills](https://code.claude.com/docs/en/skills), [DSH MCP client](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/mcp/mcp-client/README.md), [DSH Skills](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/skills.md), [DSH app boot/layers](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/boot/app-boot/README.md). DSH source was reviewed at `639ed015397290b3745d163aafe02ffee4aa3f84`.
