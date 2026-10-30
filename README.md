# EPFL OpenWork Setup

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
