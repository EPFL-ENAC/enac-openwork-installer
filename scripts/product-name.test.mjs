import { describe, expect, it } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'

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

// Every surface that ships with the product, scanned for the retired name.
const SCANNED_ROOTS = [
  'src',
  'src-tauri/src',
  'src-tauri/fixtures',
  'scripts',
  'src-cli',
  'pages',
  '.github',
]

const SCANNED_FILES = ['README.md', 'package.json', 'index.html']

// Wire identifiers: the request headers and the temp-dir prefix drop the EPFL
// prefix as well. The repository URLs are identifiers rather than display
// strings, so they keep it.
const WIRE_SOURCES = [
  ['src-tauri/src/util.rs', 'OpenWork-Installer/'],
  ['src-tauri/src/util.rs', 'openwork-installer-'],
  ['src-tauri/src/portal.rs', 'openwork-installer/'],
  ['src-cli/install.sh', 'openwork-installer-cli'],
]

const SEPARATOR = '(?:[\\s._-]|%20|%2520)*'

const RETIRED_NAME = new RegExp(['EPFL', 'OpenWork'].join(SEPARATOR), 'i')

const LEGACY_PATTERN = new RegExp(['EPFL', 'OpenWork', 'Installer'].join(SEPARATOR), 'i')

const sourceFiles = (dir) =>
  readdirSync(fileURLToPath(new URL(dir, ROOT)), { withFileTypes: true }).flatMap((entry) => {
    const path = `${dir}/${entry.name}`
    return entry.isDirectory() ? sourceFiles(path) : [path]
  })

describe('the product name', () => {
  it.each(NAME_SOURCES)('%s drops the EPFL prefix', (path, wording) => {
    const text = read(path)

    expect(text).toContain(wording)
    expect(text).not.toMatch(RETIRED_NAME)
  })

  it.each([...SCANNED_ROOTS.flatMap(sourceFiles), ...SCANNED_FILES])(
    '%s carries no retired product name',
    (path) => {
      const bytes = readFileSync(fileURLToPath(new URL(path, ROOT)))

      if (bytes.includes(0)) {
        return
      }
      expect(bytes.toString('utf8')).not.toMatch(LEGACY_PATTERN)
    },
  )

  it.each(WIRE_SOURCES)('%s carries %s without the EPFL prefix', (path, wording) => {
    const text = read(path)

    expect(text).toContain(wording)
    expect(text).not.toMatch(/epfl[-_]openwork[-_]installer/i)
  })

  it.each([
    ['src-tauri/Cargo.toml', 'name = "openwork-installer"'],
    ['src-tauri/Cargo.toml', 'name = "openwork_installer_lib"'],
    ['src-tauri/src/main.rs', 'openwork_installer_lib::run()'],
    ['package.json', '"name": "openwork-installer"'],
  ])('%s carries %s', (path, wording) => {
    expect(read(path)).toContain(wording)
  })
})
