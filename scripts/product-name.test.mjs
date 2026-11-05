import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'

const specifier = 'node:url'
const { fileURLToPath } = await import(/* @vite-ignore */ specifier)

const ROOT = new URL('..', import.meta.url)

const read = (path) => readFileSync(fileURLToPath(new URL(path, ROOT)), 'utf8')

// Every file that spells the product name out for a user, with the wording it
// must carry. The asset links encode the product name in an URL, so they are
// checked for the encoded prefix as well.
const NAME_SOURCES = [
  ['src-tauri/tauri.conf.json', 'OpenWork Installer'],
  ['src-tauri/Cargo.toml', 'OpenWork Installer'],
  ['src/i18n/en.json', 'OpenWork Installer'],
  ['src/i18n/fr.json', 'OpenWork Installer'],
  ['README.md', 'OpenWork Installer'],
  ['index.html', 'OpenWork Installer'],
  ['pages/index.html', 'OpenWork Installer'],
  ['.github/workflows/release.yml', 'OpenWork Installer'],
  ['src-cli/install.sh', 'OpenWork terminal installer'],
]

// Files that keep the historical names on purpose: repair and rerun must still
// find an install that an older build registered.
const LEGACY_NAME_SOURCES = [
  'src-tauri/src/openwork.rs',
  'src-tauri/fixtures/openwork-uninstall-entries.json',
]

// Wire identifiers: the request headers and the temp-dir prefix drop the EPFL
// prefix as well. The npm package name and the repository URLs are identifiers
// rather than display strings, so they keep it.
const WIRE_SOURCES = [
  ['src-tauri/src/util.rs', 'OpenWork-Installer/'],
  ['src-tauri/src/util.rs', 'openwork-installer-'],
  ['src-tauri/src/portal.rs', 'openwork-installer/'],
  ['src-cli/install.sh', 'openwork-installer-cli'],
]

describe('the product name', () => {
  it.each(NAME_SOURCES)('%s drops the EPFL prefix', (path, wording) => {
    const text = read(path)

    expect(text).toContain(wording)
    expect(text).not.toMatch(/EPFL OpenWork/)
    expect(text).not.toMatch(/EPFL%20OpenWork/)
  })

  it.each(LEGACY_NAME_SOURCES)('%s keeps the old name in its compatibility list', (path) => {
    expect(read(path)).toContain('EPFL OpenWork Installer')
  })

  it.each(WIRE_SOURCES)('%s carries %s without the EPFL prefix', (path, wording) => {
    const text = read(path)

    expect(text).toContain(wording)
    expect(text).not.toMatch(/epfl-openwork-installer/i)
  })
})
