# EPFL OpenWork Installer

Desktop installer for OpenWork, built with Tauri 2 and Vue 3.

## Status

Version 0.1.0 is the first release. The application is **unsigned**:

- macOS: the system blocks the first start. Right-click the application and select **Open**, then confirm.
- Windows: SmartScreen shows a warning. Select **More info**, then **Run anyway**.

Application signing and notarization are not part of this release.

## Build matrix

| Platform | Runner | Target | Artifacts |
| --- | --- | --- | --- |
| Windows x64 | `windows-latest` | default | portable `.exe` |
| macOS universal (arm64 and x64) | `macos-latest` | `universal-apple-darwin` | `.dmg` |
| Linux x64 | `ubuntu-latest` | default | `.AppImage` |
| Linux arm64 | `ubuntu-24.04-arm` | default | `.AppImage` |

Windows ships as a single portable `.exe`. There is no setup wizard and no installer. Download the file and run it. WebView2 Evergreen is already present on Windows 10 and 11.

### OpenWork does not start on macOS

OpenWork 0.18.63 can stop at start with this message:

```
OpenWork couldn't start - Unexpected error: Database is not empty and has no session table
```

The message comes from the OpenCode engine that OpenWork bundles, not from OpenWork itself. The engine reads the shared OpenCode database (`~/.local/share/opencode/*.db`), finds tables that an OpenCode v2 install created, and stops because it needs its own `session` table. The upstream fix is merged but not released, so no released OpenWork version avoids it. See [openwork#5610](https://github.com/different-ai/openwork/issues/5610) and [openwork#6662](https://github.com/different-ai/openwork/pull/6662).

The OpenWork step of this setup detects the condition. It explains it, and with the consent checkbox it moves the database file and its `-wal` and `-shm` files to `<name>.bak-<timestamp>` next to the original. Nothing is deleted, and OpenWork creates a fresh database on its next start. Ceiling: the check exists only in this setup, and it can be removed when a released OpenWork version contains #6662.

## Linux

The AppImage needs the system WebKitGTK library and FUSE. On a Debian or Ubuntu system, install them with:

```sh
sudo apt-get install libwebkit2gtk-4.1-0 libfuse2
```

On Ubuntu 24.04, Debian 13 and newer, the FUSE package is named `libfuse2t64`:

```sh
sudo apt-get install libwebkit2gtk-4.1-0 libfuse2t64
```

Then make the file executable and run it:

```sh
chmod +x ./*.AppImage
./EPFL*.AppImage
```

If FUSE is not available on the machine, run the AppImage with `--appimage-extract-and-run`. The arm64 build needs the same libraries.

## Updates

There is no self-update. To update, download the new release from GitHub Releases and replace the old file. On Linux, download the new `.AppImage` and replace the old one.

## Release process

1. Increment the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`. Use the same value in all three files.
2. Commit the change.
3. Create an annotated tag with the same version, prefixed with `v`:

   ```sh
   git tag -a v0.1.0 -m "v0.1.0"
   git push origin v0.1.0
   ```

4. The `Release` workflow builds all four platforms and publishes the GitHub Release.
5. After the workflow succeeds, verify that the release contains the portable `.exe`, the `.dmg` and the two `.AppImage` files.

## Live test checklist

Run this checklist on the EPFL network or VPN, on a Windows x64 machine, on a macOS machine (Apple silicon and Intel) and on an EPFL-network Linux x64 machine. Use a release build from GitHub Releases, or a local build of the same commit. Use a test EPFL account.

### Fresh RCP key

- [ ] Start the app and confirm the eight steps appear in order. A later step stays locked until the previous step passes.
- [ ] Network check passes. Disconnect the VPN and confirm the app shows the VPN link and a working retry.
- [ ] OpenCode step detects an existing install and skips it. On a clean machine it installs OpenCode without an admin prompt.
- [ ] RCP key step opens the portal window. Complete the Entra ID login and the MFA prompt.
- [ ] Create a key named `OpenWork`. Confirm the token is masked by default, the reveal button works, and the clipboard warning appears after copy.
- [ ] Provider configuration step shows `~/.config/opencode/opencode.json` and its `.bak` path, then reports success.
- [ ] Smoke test passes. A failure shows a readable detail and no key.
- [ ] OpenWork step detects an existing install and skips it. On a clean machine it installs OpenWork without an admin prompt.
- [ ] Summary shows both versions, the config paths, the key name and the smoke result.
- [ ] Launch button opens the installed OpenWork app.

### Existing OpenWork key

- [ ] Create an `OpenWork` key in the portal before the run. Confirm that this is the expected first-user case.
- [ ] RCP key step detects the existing key and asks for the token. Paste the existing token with the manual field.
- [ ] The portal still has exactly the same keys after the run. No key is renamed, revoked or deleted.
- [ ] Provider configuration writes the existing token, and the smoke test passes.
- [ ] Re-run the provider configuration step. It reports no change, does not overwrite the backup, and keeps other providers and user settings.

### Installer checks, all platforms

- [ ] Windows: the portable `.exe` starts with no admin prompt. A second run detects the existing OpenWork install and does not overwrite it.
- [ ] macOS: the `.dmg` copies OpenWork to Applications. A second run detects the install and does not overwrite it.
- [ ] Linux x64 and arm64: the `.AppImage` starts after `chmod +x`, with the prerequisites installed. A second run detects the existing OpenWork install and does not overwrite it.
- [ ] macOS first start: right-click, select Open, and confirm. Windows first start: SmartScreen, More info, Run anyway. The README states both workarounds.
- [ ] macOS with an old OpenCode database in `~/.local/share/opencode`: the OpenWork step shows the database warning. The repair button stays disabled until the consent checkbox is set. After the repair the backup files exist, and OpenWork starts.

### Secret checks

- [ ] Search the OpenCode log directory (`~/.local/share/opencode/log/`) for the token. It must not appear.
- [ ] Search the app output and the browser or webview console for the token, the `sessionid` cookie and the `csrf` value. None may appear.
- [ ] The token appears only inside `~/.config/opencode/opencode.json`, in the RCP provider settings.
- [ ] The backup file and the other config files contain no new secret.

### Release checks

- [ ] The GitHub Release holds the portable `.exe`, the `.dmg` and the two `.AppImage` files.
- [ ] All four build jobs finished green.
- [ ] The `.exe` runs on a machine without a previous install and without an admin prompt.

## Development

```sh
npm install
npm run tauri dev
```

## Tests

```sh
npm run test:unit
cd src-tauri && cargo test
```

The `Test` workflow runs these commands on each pull request.
