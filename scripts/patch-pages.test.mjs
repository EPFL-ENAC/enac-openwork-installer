import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'

import { assetUrl, patchPage, pickAssets } from './patch-pages.mjs'

const specifier = 'node:url'
const { fileURLToPath } = await import(/* @vite-ignore */ specifier)

const PAGE = fileURLToPath(new URL('../pages/index.html', import.meta.url))
const README = fileURLToPath(new URL('../README.md', import.meta.url))

const ASSETS = [
  'OpenWork-Installer-aarch64.AppImage',
  'OpenWork-Installer-amd64.AppImage',
  'OpenWork-Installer-universal.dmg',
  'OpenWork-Installer-x64.exe',
  'install.sh',
  'SHA256SUMS',
]

const STABLE = ASSETS.filter((name) => name.startsWith('OpenWork-Installer-'))

const page = readFileSync(PAGE, 'utf8')
const readme = readFileSync(README, 'utf8')

describe('pickAssets', () => {
  it('maps every platform role to the matching release asset', () => {
    expect(pickAssets(ASSETS)).toEqual({
      windows: 'OpenWork-Installer-x64.exe',
      macos: 'OpenWork-Installer-universal.dmg',
      'linux-x64': 'OpenWork-Installer-amd64.AppImage',
      'linux-arm64': 'OpenWork-Installer-aarch64.AppImage',
    })
  })

  it('fails on a release without a linux x64 AppImage', () => {
    const names = ASSETS.filter((name) => !name.includes('amd64'))

    expect(() => pickAssets(names)).toThrow(/linux-x64/)
  })

  it('fails instead of guessing between two assets for one role', () => {
    const names = [...ASSETS, 'OpenWork Installer_0.1.0_arm64.AppImage']

    expect(() => pickAssets(names)).toThrow(/linux-arm64/)
  })

  it('fails on a release that still carries two versioned assets for one role', () => {
    const names = ASSETS.filter((name) => !name.endsWith('.dmg')).concat(
      'OpenWork Installer_0.1.0_universal.dmg',
      'OpenWork Installer_0.1.0_aarch64.dmg',
    )

    expect(() => pickAssets(names)).toThrow(/macos/)
  })
})

describe('assetUrl', () => {
  it('returns a stable name without escaping', () => {
    expect(assetUrl('OpenWork-Installer-x64.exe')).toBe(
      'https://github.com/EPFL-ENAC/enac-openwork-installer/releases/latest/download/' +
        'OpenWork-Installer-x64.exe',
    )
    expect(assetUrl('OpenWork-Installer-x64.exe')).not.toContain('%')
  })
})

describe('patchPage', () => {
  const assets = pickAssets(ASSETS)
  const patched = patchPage(page, { version: 'v0.1.2', assets })

  it('leaves the stable download hrefs of the repository page untouched', () => {
    const hrefs = [...page.matchAll(/<a\b[^>]*data-asset="([a-z0-9-]+)"[^>]*>/g)].map((tag) => {
      const href = tag[0].match(/href="([^"]*)"/)
      return [tag[1], href?.[1]]
    })

    expect(Object.fromEntries(hrefs)).toEqual({
      windows: assetUrl('OpenWork-Installer-x64.exe'),
      macos: assetUrl('OpenWork-Installer-universal.dmg'),
      'linux-x64': assetUrl('OpenWork-Installer-amd64.AppImage'),
      'linux-arm64': assetUrl('OpenWork-Installer-aarch64.AppImage'),
    })
    expect(patched).toBe(
      page.replace(
        /(<span [^>]*data-release-version[^>]*>)[^<]*/,
        '$1v0.1.2',
      ),
    )
  })

  it('shows the tag in the version marker and nowhere else as a stale number', () => {
    const version = patched.match(/<span [^>]*data-release-version[^>]*>([^<]*)</)

    expect(page).toContain('<span data-release-version>0.1.0</span>')
    expect(version?.[1]).toBe('v0.1.2')
  })

  it('keeps the install script and checksum links permanent', () => {
    expect(patched.match(/releases\/latest\/download\/install\.sh/g)).toHaveLength(2)
    expect(patched.match(/releases\/latest\/download\/SHA256SUMS/g)).toHaveLength(1)
  })

  it('fails when a page href drifted from the stable name of its role', () => {
    const drifted = page.replace('OpenWork-Installer-x64.exe', 'OpenWork Installer_0.1.0_x64.exe')

    expect(() => patchPage(drifted, { version: 'v0.1.2', assets })).toThrow(/windows/)
  })

  it('fails when the page lost the hook of a role', () => {
    const broken = page.replace('data-asset="macos"', 'data-asset="mac"')

    expect(() => patchPage(broken, { version: 'v0.1.2', assets })).toThrow(/macos/)
  })
})

describe('the stable links', () => {
  it.each(STABLE)('%s is linked from the page and the README', (name) => {
    expect(page).toContain(`releases/latest/download/${name}`)
    expect(readme).toContain(`releases/latest/download/${name}`)
  })

  it('links no versioned asset name from the page or the README', () => {
    const versioned = /releases\/latest\/download\/[^"'\s)<>]*_\d+\.\d+\.\d+_/

    expect(page).not.toMatch(versioned)
    expect(readme).not.toMatch(versioned)
  })
})

describe('the page itself', () => {
  it('stacks every platform section until the script hides them', () => {
    const tabs = page.match(/<div class="tabs"[^>]*>/)
    const panels = page.match(/<section class="panel"[^>]*>/g) ?? []

    expect(tabs?.[0]).toContain('hidden')
    expect(panels).toHaveLength(3)
    expect(panels.filter((tag) => tag.includes('hidden'))).toEqual([])
  })

  it('keeps every raw color inside the token block', () => {
    const start = page.indexOf(':root {')
    const block = page.slice(start, page.indexOf('}', start))
    const outside = page.slice(page.indexOf('}', start)).match(/#[0-9a-f]{3,8}\b/gi) ?? []

    expect(start).toBeGreaterThan(-1)
    expect(block.match(/#[0-9a-f]{3,8}\b/gi)?.length).toBeGreaterThan(10)
    expect(outside).toEqual([])
  })
})
