import { describe, expect, it } from 'vitest'

const specifier = 'node:fs'

type Dirent = { name: string; isDirectory: () => boolean }

const fs = (await import(/* @vite-ignore */ specifier)) as {
  readdirSync: (url: URL, options: { withFileTypes: true }) => Dirent[]
  readFileSync: (url: URL, encoding: 'utf8') => string
}

const SRC = new URL('..', import.meta.url)
const TOKEN_FILE = 'epfl-tokens.css'
const SCANNED = ['.vue', '.css', '.scss', '.ts']
const HEX = /#[0-9a-fA-F]{3,8}\b/g

// This guard spells the banned values, so it is the one file the hex scan skips.
const GUARD_FILE = 'palette.test.ts'

const BANNED = [
  '#7ED321',
  '#699B32',
  '#F5A623',
  '#4A90E2',
  '#00A79F',
  '#E30713',
  '#92000E',
  '#E8F1FB',
  '#B8D4F0',
  '#1E4A72',
  '#ECFAE0',
  '#C8EEA0',
  '#3A5E1A',
  '#FDF2DC',
  '#F6DBA0',
  '#7A560F',
]

const sources = (dir: URL, found: URL[] = []): URL[] => {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const isDir = entry.isDirectory()
    const child = new URL(entry.name + (isDir ? '/' : ''), dir)
    if (isDir) sources(child, found)
    else if (entry.name !== GUARD_FILE && SCANNED.some((ext) => entry.name.endsWith(ext))) {
      found.push(child)
    }
  }
  return found
}

const relative = (url: URL): string => url.pathname.slice(SRC.pathname.length)
const read = (url: URL): string => fs.readFileSync(url, 'utf8')
const files = sources(SRC)

describe('EPFL palette guard', () => {
  it('keeps every raw hex inside the token file', () => {
    const offenders = files
      .filter((file) => !file.pathname.endsWith(TOKEN_FILE))
      .flatMap((file) => (read(file).match(HEX) ?? []).map((hex) => `${relative(file)}: ${hex}`))

    expect(offenders).toEqual([])
  })

  it('defines no green, orange, blue or leman value in the token file', () => {
    const css = read(new URL(`./${TOKEN_FILE}`, import.meta.url)).toLowerCase()
    const found = BANNED.filter((hex) => css.includes(hex.toLowerCase()))

    expect(found).toEqual([])
  })

  it('drops the print red from the tree', () => {
    const offenders = files
      .filter((file) => read(file).toLowerCase().includes('#e30713'))
      .map(relative)

    expect(offenders).toEqual([])
  })

  it('defines every status surface from the red and grey families', () => {
    const css = read(new URL(`./${TOKEN_FILE}`, import.meta.url))
    const statusTokens = css.match(/--(success|warning|info|leman|blue|orange|green)[a-z-]*:/g) ?? []

    expect(statusTokens).toEqual([])
  })
})
