#!/usr/bin/env node
// Rewrites pages/index.html for GitHub Pages. The release asset names embed the
// build version and the tag does not, so the deploy job takes the names from the
// release metadata and patches them into the page it publishes.
//
// Usage: node scripts/patch-pages.mjs <release.json> <page.html> [...]
// release.json is the output of: gh release view <tag> --json tagName,assets
import { readFile, writeFile } from 'node:fs/promises'

const BASE = 'https://github.com/EPFL-ENAC/enac-openwork-installer/releases/latest/download/'

const ARM = /(aarch64|arm64)/i

const MATCHERS = {
  windows: (name) => /\.exe$/i.test(name),
  macos: (name) => /\.dmg$/i.test(name),
  'linux-x64': (name) => /\.AppImage$/i.test(name) && !ARM.test(name),
  'linux-arm64': (name) => /\.AppImage$/i.test(name) && ARM.test(name),
}

export const assetUrl = (name) => BASE + encodeURIComponent(name)

export function pickAssets(names) {
  const picked = {}

  for (const [role, matches] of Object.entries(MATCHERS)) {
    const found = names.filter(matches)
    if (found.length !== 1) {
      throw new Error(`expected one release asset for ${role}, found ${found.length}: ${found}`)
    }
    picked[role] = found[0]
  }

  return picked
}

export function patchPage(html, { version, assets }) {
  let patched = html

  for (const [role, name] of Object.entries(assets)) {
    const tag = new RegExp(`<a\\b[^>]*\\bdata-asset="${role}"[^>]*>`, 'g')
    const found = patched.match(tag) ?? []
    if (found.length !== 1) {
      throw new Error(`the page needs one download link for ${role}, found ${found.length}`)
    }
    patched = patched.replace(tag, (anchor) =>
      anchor.replace(/href="[^"]*"/, `href="${assetUrl(name)}"`),
    )
  }

  const marker = /(<[a-z]+[^>]*\bdata-release-version\b[^>]*>)[^<]*(<)/
  const matches = patched.match(new RegExp(marker.source, 'g')) ?? []
  if (matches.length !== 1) {
    throw new Error(`the page needs one data-release-version marker, found ${matches.length}`)
  }

  return patched.replace(marker, `$1${version}$2`)
}

const main = async ([releasePath, ...pagePaths]) => {
  if (!releasePath || pagePaths.length === 0) {
    throw new Error('usage: patch-pages.mjs <release.json> <page.html> [...]')
  }

  const release = JSON.parse(await readFile(releasePath, 'utf8'))
  const names = (release.assets ?? []).map((asset) =>
    typeof asset === 'string' ? asset : asset.name,
  )
  const version = release.tagName

  if (!version) throw new Error(`${releasePath} has no tagName`)

  const assets = pickAssets(names)

  for (const pagePath of pagePaths) {
    await writeFile(pagePath, patchPage(await readFile(pagePath, 'utf8'), { version, assets }))
  }

  console.log(`patched ${pagePaths.join(' ')} for ${version}: ${JSON.stringify(assets)}`)
}

const isCli = async () => {
  const { pathToFileURL } = await import('node:url')

  return Boolean(process.argv[1]) && import.meta.url === pathToFileURL(process.argv[1]).href
}

isCli().then((run) => {
  if (!run) return

  main(process.argv.slice(2)).catch((error) => {
    console.error(`::error::${error.message}`)
    process.exit(1)
  })
})
