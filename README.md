# EPFL OpenWork Installer

Desktop installer for OpenWork and configuration to use EPFL RCP's AI as a Service inference, built with Tauri 2 and Vue 3.


## Build matrix

| Platform | Runner | Target | Artifacts |
| --- | --- | --- | --- |
| Windows x64 | `windows-latest` | default | portable `.exe` |
| macOS universal (arm64 and x64) | `macos-latest` | `universal-apple-darwin` | `.dmg` |
| Linux x64 | `ubuntu-latest` | default | `.AppImage` |
| Linux arm64 | `ubuntu-24.04-arm` | default | `.AppImage` |



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
