# Application updates

TodoList uses the Tauri updater for application-update integrity. This signature is separate from Windows Authenticode publisher signing.

## User flow

1. A production desktop build performs a non-blocking update check after launch, then throttles background focus/visibility checks to once per six hours. The user can also check from Settings. Browser previews never contact the updater, and development builds have no production endpoint.
2. When a newer version exists, the sidebar Settings row shows a quiet update badge and Settings shows both installed and available versions. Offline or failed checks never claim that the current build is latest.
3. Download and installation begin only after the user clicks the update action. Installation starts only after the Tauri signature has been verified; a network or signature failure does not claim that a retryable installer exists.
4. On Windows, the installer stops only the TodoList GUI and MCP executables resolved inside the selected installation directory. Codex, development builds and other installations remain running.
5. The installer stages the previous GUI and MCP as one transaction. While replacement is in progress, an installation marker makes an independently restarted MCP exit with a retryable update-in-progress error.
6. Success is recorded only after the installed GUI version and MCP build identity both match. After that commit point, cache cleanup cannot roll the installation back or report it as failed. A hidden helper waits for the installer process to exit, rechecks both components, and removes only that automatic-update installer and its owned cache directory. If cleanup is temporarily blocked, the installed result remains successful and TodoList retries the owned cleanup on a later startup.

Downgrades are disabled by the app. Windows uses the updater's passive installer mode, which keeps installation visible without adding extra choices to the update flow.

If installation fails before the commit point, both components are rolled back together. Recovery is based on the actual owned backup files, so an interruption before the first move, between component moves, or between a move and transaction-state persistence cannot delete an original that was never staged. The verified installer remains under `%TEMP%\TodoList-<version>-updater-<random>\TodoList-<version>-installer.exe`, and Explorer selects it once for each actual retry that fails so the user can retry manually; restarting TodoList does not repeat the selection. Settings also offers retry, open-location and explicit cleanup actions. Silent/passive updates do not show an additional modal failure box. Cancellation is recorded separately, retains the installer, and does not open Explorer. A download failure has no installer to open. Manual downloads, unmarked directories, unexpected files and caches belonging to another installation are never reclaimed by this flow.

## Release configuration

The public repository is `patrickzw1/TodoList`. Windows release builds merge
`src-tauri/tauri.release.conf.json`, which contains the public verification key and
the GitHub `latest.json` endpoint. It also restores the production application identifier/name, enables the `production` feature for both the desktop and sidecar, and selects the reviewed TodoList NSIS template and lifecycle hooks. It contains no private signing material. Ordinary local builds use the independent development database even when compiled with release optimizations.

GitHub Actions is the primary release environment:

- `.github/workflows/ci.yml` runs TypeScript, frontend application, build-channel, Sites and Rust workspace checks for pushes to `main`, pull requests and explicit manual CI runs. It has read-only repository permission and never receives signing secrets.
- `.github/workflows/release.yml` builds and publishes only after a stable `vMAJOR.MINOR.PATCH` tag is pushed. Its optional manual dispatch does not create a tag: the supplied tag must already exist, point to the checked-out commit, be contained in `origin/main` and exactly match `package.json`.
- The release job tests the tagged source, builds the production GUI, production MCP sidecar and signed NSIS package on Windows, then verifies the updater signature, manifest, checksums, executable identities, PE subsystems and private build-path scan.
- The four expected assets are uploaded to a draft first. The workflow downloads them again and compares their sizes and SHA-256 hashes before making the release public. Any earlier failure leaves no public release. A published tag is never overwritten.

The release workflow requires exactly these repository Actions secrets under
**Settings → Secrets and variables → Actions**:

- `TAURI_SIGNING_PRIVATE_KEY`: the complete private Tauri updater key value.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the password for that private key.

Do not put either value in workflow YAML, repository variables, logs or release
assets. Pull-request and ordinary CI jobs do not reference these secrets. The
public updater key remains in `src-tauri/tauri.release.conf.json`.

To release a version, first update every package/Tauri/Rust version and its
`docs/RELEASE_NOTES_v<version>.md`, merge the tested commit to `main`, then create
and push the matching annotated tag. Pushing the tag is the production-build
trigger; an ordinary source push only runs CI. If a run needs retrying without
moving the tag, manually dispatch **Release** with that existing tag. A failed
upload may retain a workflow-owned draft; the retry safely replaces only the four
known assets in that marked draft. An unowned draft or any public release causes
the workflow to stop for manual review.

The build creates `TodoList_<version>_x64-setup.exe`, its `.sig`, `latest.json`
and `SHA256SUMS.txt` under `target/package-build/release/bundle/nsis`. A normal
release is required for GitHub's `/releases/latest` endpoint; prereleases do not
become that endpoint.

The signing build remaps the maintainer's checkout and user-directory paths in Rust output and uses a neutral Windows debug-record path. This keeps personal build paths out of the distributed executables. Do not upload PDBs, databases, managed user files, or internal design-session records with a release.

Keep production outputs out of `target/development`: the sidecar preparation script refuses that destination for production builds, and the signing script selects `target/package-build` automatically.

The optional maintainer fallback keeps its encrypted private key outside the repository under
`%LOCALAPPDATA%\TodoListRelease\signing\updater.key`; its random password is stored
beside it in `updater.password.dpapi`, protected with Windows DPAPI for the current
user. The build script places credentials in its process environment only and
restores the original environment afterward. Neither private file belongs in Git
or release assets. DPAPI is tied to the Windows account: before moving or
reinstalling this machine, retain an independently encrypted offline backup of
the key and its decrypted password. `scripts/build-release.ps1` prefers the two
explicit environment variables in GitHub Actions and falls back to these DPAPI
files only during an intentional local maintainer build.

Do not close Codex for an update. The Windows installer coordinates the installed sidecar by its resolved executable path and leaves Codex itself running; a sidecar restart during replacement receives the update-in-progress error and can retry after installation completes.

## Setting up another release environment

Development and web-preview builds intentionally have no update endpoint. Copy `src-tauri/tauri.release.conf.example.json` to a release-only configuration after the GitHub repository and update public key exist, replace its public values, and merge it when building:

```powershell
npm run tauri build -- --config src-tauri/tauri.release.conf.json
```

Generate the Tauri updater key outside this repository. Keep the private key and its password in CI secrets; never commit them or place them in the install package. The public key belongs in the release configuration. Release builds require both `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in the build environment.

The release pipeline must publish the generated updater artifact, its `.sig`, and a matching `latest.json`. Losing the private key prevents shipping trusted updates to existing installations, so retain an encrypted offline backup.
