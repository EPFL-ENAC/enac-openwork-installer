import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'

import { assetUrl, patchPage, pickAssets } from './patch-pages.mjs'

const specifier = 'node:url'
const { fileURLToPath } = await import(/* @vite-ignore */ specifier)

const PAGE = fileURLToPath(new URL('../pages/index.html', import.meta.url))

const ASSETS = [
  'EPFL OpenWork Installer_0.1.0_aarch64.AppImage',
  'EPFL OpenWork Installer_0.1.0_amd64.AppImage',
  'EPFL OpenWork Installer_0.1.0_universal.dmg',
  'EPFL OpenWork Installer_0.1.0_x64.exe',
  'install.sh',
  'SHA256SUMS',
]

const page = readFileSync(PAGE, 'utf8')

describe('pickAssets', () => {
  it('maps every platform role to the matching release asset', () => {
    expect(pickAssets(ASSETS)).toEqual({
      windows: 'EPFL OpenWork Installer_0.1.0_x64.exe',
      macos: 'EPFL OpenWork Installer_0.1.0_universal.dmg',
      'linux-x64': 'EPFL OpenWork Installer_0.1.0_amd64.AppImage',
      'linux-arm64': 'EPFL OpenWork Installer_0.1.0_aarch64.AppImage',
    })
  })

  it('takes the asset names, not the tag, as the version of the files', () => {
    const names = ASSETS.filter((name) => !name.endsWith('.exe')).concat(
      'EPFL OpenWork Installer_0.9.9_x64.exe',
    )

    expect(pickAssets(names).windows).toBe('EPFL OpenWork Installer_0.9.9_x64.exe')
  })

  it('fails on a release without a linux x64 AppImage', () => {
    const names = ASSETS.filter((name) => !name.includes('amd64'))

    expect(() => pickAssets(names)).toThrow(/linux-x64/)
  })

  it('fails instead of guessing between two assets for one role', () => {
    const names = [...ASSETS, 'EPFL OpenWork Installer_0.1.0_arm64.AppImage']

    expect(() => pickAssets(names)).toThrow(/linux-arm64/)
  })
})

describe('assetUrl', () => {
  it('escapes the spaces of the product name', () => {
    expect(assetUrl('EPFL OpenWork Installer_0.1.0_x64.exe')).toBe(
      'https://github.com/EPFL-ENAC/enac-openwork-installer/releases/latest/download/' +
        'EPFL%20OpenWork%20Installer_0.1.0_x64.exe',
    )
  })
})

describe('patchPage', () => {
  const patched = patchPage(page, { version: 'v0.1.2', assets: pickAssets(ASSETS) })

  it('points every download hook at the release asset of its role', () => {
    const hrefs = [...patched.matchAll(/<a\b[^>]*data-asset="([a-z0-9-]+)"[^>]*>/g)].map((tag) => {
      const href = tag[0].match(/href="([^"]*)"/)
      return [tag[1], href?.[1]]
    })

    expect(Object.fromEntries(hrefs)).toEqual({
      windows: assetUrl('EPFL OpenWork Installer_0.1.0_x64.exe'),
      macos: assetUrl('EPFL OpenWork Installer_0.1.0_universal.dmg'),
      'linux-x64': assetUrl('EPFL OpenWork Installer_0.1.0_amd64.AppImage'),
      'linux-arm64': assetUrl('EPFL OpenWork Installer_0.1.0_aarch64.AppImage'),
    })
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

  it('fails when the page lost the hook of a role', () => {
    const broken = page.replace('data-asset="macos"', 'data-asset="mac"')

    expect(() => patchPage(broken, { version: 'v0.1.2', assets: pickAssets(ASSETS) })).toThrow(
      /macos/,
    )
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
