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
| Windows x64 | `windows-latest` | default | NSIS `.exe`, updater `.nsis.zip` |
| macOS universal (arm64 and x64) | `macos-latest` | `universal-apple-darwin` | `.dmg`, updater `.app.tar.gz` |

Each job also uploads its signatures and a shared `latest.json` file to the GitHub Release.

## Auto-updater

The application uses `tauri-plugin-updater`. The endpoint is:

```
https://github.com/EPFL-ENAC/enac-openwork-installer/releases/latest/download/latest.json
```

The updater works only for an installed release build. It does not work in `tauri dev`.

### Signing key

The public key is in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). Generate the keypair one time:

```sh
npm run tauri signer generate -- -w ~/.tauri/enac-openwork-installer.key
```

Store these repository secrets in GitHub (**Settings > Secrets and variables > Actions**):

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Contents of `~/.tauri/enac-openwork-installer.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The password that you entered during key generation |

Never commit the private key. If you lose the private key or the password, existing installations cannot receive updates. In that case, generate a new keypair and put the new public key in `tauri.conf.json` in the next release.

## Release process

1. Increment the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`. Use the same value in all three files.
2. Commit the change.
3. Create an annotated tag with the same version, prefixed with `v`:

   ```sh
   git tag -a v0.1.0 -m "v0.1.0"
   git push origin v0.1.0
   ```

4. The `Release` workflow builds both platforms and publishes the GitHub Release. The tag must match the application version, because `latest.json` uses the application version.
5. After the workflow succeeds, verify that the release contains the `.exe`, the `.dmg`, the updater artifacts, and `latest.json`.

The `Release` workflow needs both signing secrets. If `TAURI_SIGNING_PRIVATE_KEY` is missing, the build fails, because the configuration contains a public key. The private key must match that public key.

## Live test checklist

Run this checklist on the EPFL network or VPN, on a Windows x64 machine and on a macOS machine (Apple silicon and Intel). Use a release build from GitHub Releases, or a local build of the same commit. Use a test EPFL account.

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

### Installer checks, both platforms

- [ ] Windows: the `.exe` installs per user with no admin prompt. A second run detects the install and does not overwrite it.
- [ ] macOS: the `.dmg` copies OpenWork to Applications. A second run detects the install and does not overwrite it.
- [ ] macOS first start: right-click, select Open, and confirm. Windows first start: SmartScreen, More info, Run anyway. The README states both workarounds.

### Secret checks

- [ ] Search the OpenCode log directory (`~/.local/share/opencode/log/`) for the token. It must not appear.
- [ ] Search the app output and the browser or webview console for the token, the `sessionid` cookie and the `csrf` value. None may appear.
- [ ] The token appears only inside `~/.config/opencode/opencode.json`, in the RCP provider settings.
- [ ] The backup file and the other config files contain no new secret.

### Release checks

- [ ] The GitHub Release holds the NSIS `.exe`, the `.dmg`, the updater artifacts and `latest.json`.
- [ ] Both build jobs used the signing secrets and finished green.
- [ ] The updater endpoint in `latest.json` points at the same release.

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
