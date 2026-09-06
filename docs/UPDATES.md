# Application updates

TodoList uses the Tauri updater for application-update integrity. This signature is separate from Windows Authenticode publisher signing.

## User flow

1. The user opens Settings and checks for updates.
2. TodoList reads the configured HTTPS update endpoint.
3. A newer version is downloaded first. Installation begins only after the Tauri signature has been verified; a network or signature failure does not claim that a retryable installer exists.
4. On Windows, the installer stops only the TodoList GUI and MCP executables resolved inside the selected installation directory. Codex, development builds and other installations remain running.
5. The installer stages the previous GUI and MCP as one transaction. While replacement is in progress, an installation marker makes an independently restarted MCP exit with a retryable update-in-progress error.
6. Success is recorded only after the installed GUI version and MCP build identity both match. After that commit point, cache cleanup cannot roll the installation back or report it as failed. A hidden helper waits for the installer process to exit, rechecks both components, and removes only that automatic-update installer and its owned cache directory. If cleanup is temporarily blocked, the installed result remains successful and TodoList retries the owned cleanup on a later startup.

Downgrades are disabled by the app. Windows uses the updater's passive installer mode, which keeps installation visible without adding extra choices to the update flow.

If installation fails before the commit point, both components are rolled back together. Recovery is based on the actual owned backup files, so an interruption before the first move, between component moves, or between a move and transaction-state persistence cannot delete an original that was never staged. The verified installer remains under `%TEMP%\TodoList-<version>-updater-<random>\TodoList-<version>-installer.exe`, and Explorer selects it once for each actual retry that fails so the user can retry manually; restarting TodoList does not repeat the selection. Settings also offers retry, open-location and explicit cleanup actions. Silent/passive updates do not show an additional modal failure box. Cancellation is recorded separately, retains the installer, and does not open Explorer. A download failure has no installer to open. Manual downloads, unmarked directories, unexpected files and caches belonging to another installation are never reclaimed by this flow.

## Release configuration

The public repository is `patrickzw1/TodoList`. Windows release builds merge
`src-tauri/tauri.release.conf.json`, which contains the public verification key and
the GitHub `latest.json` endpoint. It also restores the production application identifier/name, enables the `production` feature for both the desktop and sidecar, and selects the reviewed TodoList NSIS template and lifecycle hooks. It contains no private signing material. Ordinary local builds use the independent development database even when compiled with release optimizations.

On the release maintainer's Windows machine, run:

```powershell
./scripts/build-release.ps1
```

This builds the signed NSIS installer in `target/package-build/release/bundle/nsis`
and creates its signature, `latest.json`, and `SHA256SUMS.txt`. Upload these four
files to the matching `v<version>` GitHub release. A normal release is required
for GitHub's `/releases/latest` endpoint; prereleases do not become that endpoint.

The signing build remaps the maintainer's checkout and user-directory paths in Rust output and uses a neutral Windows debug-record path. This keeps personal build paths out of the distributed executables. Do not upload PDBs, databases, managed user files, or internal design-session records with a release.

Keep production outputs out of `target/development`: the sidecar preparation script refuses that destination for production builds. The signing script selects `target/package-build` automatically. For an unsigned local production build check, set `CARGO_TARGET_DIR` to `target/package-build` and run `npm run build:desktop -- --no-bundle --config src-tauri/tauri.release.conf.json`.

The encrypted private key is outside the repository under
`%LOCALAPPDATA%\TodoListRelease\signing\updater.key`; its random password is stored
beside it in `updater.password.dpapi`, protected with Windows DPAPI for the current
user. The build script places credentials in its process environment only and
restores the original environment afterward. Neither private file belongs in Git
or release assets. DPAPI is tied to the Windows account: before moving or
reinstalling this machine, retain an independently encrypted offline backup of
the key and its decrypted password. CI signing remains a separate setup step.

Do not close Codex for an update. The Windows installer coordinates the installed sidecar by its resolved executable path and leaves Codex itself running; a sidecar restart during replacement receives the update-in-progress error and can retry after installation completes.

## Setting up another release environment

Development and web-preview builds intentionally have no update endpoint. Copy `src-tauri/tauri.release.conf.example.json` to a release-only configuration after the GitHub repository and update public key exist, replace its public values, and merge it when building:

```powershell
npm run tauri build -- --config src-tauri/tauri.release.conf.json
```

Generate the Tauri updater key outside this repository. Keep the private key and its password in CI secrets; never commit them or place them in the install package. The public key belongs in the release configuration. Release builds require `TAURI_SIGNING_PRIVATE_KEY` and, when applicable, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in the build environment.

The release pipeline must publish the generated updater artifact, its `.sig`, and a matching `latest.json`. Losing the private key prevents shipping trusted updates to existing installations, so retain an encrypted offline backup.
