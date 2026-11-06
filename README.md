# OpenWork Installer

Desktop installer for OpenWork, with configuration to use EPFL RCP's AI as a Service (AIasS) inference, built with Tauri 2 and Vue 3.

It does the following:

- Create an API key for LLM inference on EPFL RCP AIaaS (must be on EPFL network or VPN)
- Install OpenWork
- Configure OpenWork to use EPFL RCP AIaaS models ("Large" and "Small")


## Quick install

### Downloads

| Platform | Installer |
| --- | --- |
| Windows x64 | [OpenWork-Installer-x64.exe](https://github.com/EPFL-ENAC/enac-openwork-installer/releases/latest/download/OpenWork-Installer-x64.exe) |
| macOS universal (Apple silicon and Intel) | [OpenWork-Installer-universal.dmg](https://github.com/EPFL-ENAC/enac-openwork-installer/releases/latest/download/OpenWork-Installer-universal.dmg) |
| Linux x64 | [OpenWork-Installer-amd64.AppImage](https://github.com/EPFL-ENAC/enac-openwork-installer/releases/latest/download/OpenWork-Installer-amd64.AppImage) |
| Linux arm64 | [OpenWork-Installer-aarch64.AppImage](https://github.com/EPFL-ENAC/enac-openwork-installer/releases/latest/download/OpenWork-Installer-aarch64.AppImage) |

These links always point at the latest release. Check a download against [SHA256SUMS](https://github.com/EPFL-ENAC/enac-openwork-installer/releases/latest/download/SHA256SUMS), and find every release with its notes on the [releases page](https://github.com/EPFL-ENAC/enac-openwork-installer/releases).

### Or install from a terminal

The shell installer does everything the desktop installer does, from one command. It works on Linux x64 and arm64, and on macOS:

```sh
curl -fsSL https://github.com/EPFL-ENAC/enac-openwork-installer/releases/latest/download/install.sh | bash
```

The installer is interactive, it needs a terminal. It is safe to run twice, it resumes where it stopped. Review the script before running it: it is plain bash and needs `curl` and `sqlite3`.


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
./OpenWork*.AppImage
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
5. After the workflow succeeds, verify that the release contains the portable `.exe`, the `.dmg`, the two `.AppImage` files, `install.sh` and `SHA256SUMS`, all with their version-less name.
6. The same workflow publishes the download page at https://epfl-enac.github.io/enac-openwork-installer/. Enable Pages once in the repository settings, with "GitHub Actions" as the source.


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
