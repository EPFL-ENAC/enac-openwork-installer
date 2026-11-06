import { describe, expect, it } from 'vitest'
import { execFileSync } from 'node:child_process'
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const specifier = 'node:url'
const { fileURLToPath } = await import(/* @vite-ignore */ specifier)

const WORKFLOW = fileURLToPath(new URL('../.github/workflows/release.yml', import.meta.url))

const RELEASED = [
  'OpenWork Installer_0.1.2_aarch64.AppImage',
  'OpenWork Installer_0.1.2_amd64.AppImage',
  'OpenWork Installer_0.1.2_universal.app.tar.gz',
  'OpenWork Installer_0.1.2_universal.dmg',
  'OpenWork Installer_0.1.2_x64.exe',
  'SHA256SUMS',
  'install.sh',
]

const KEPT = ['install.sh', 'SHA256SUMS']

const STABLE = [
  'OpenWork-Installer-aarch64.AppImage',
  'OpenWork-Installer-amd64.AppImage',
  'OpenWork-Installer-universal.dmg',
  'OpenWork-Installer-x64.exe',
]

const GH_STUB = `#!/usr/bin/env bash
set -euo pipefail
cmd="$1 $2"; shift 2
case "$cmd" in
  "release download")
    dir=""
    while [ $# -gt 0 ]; do case "$1" in --dir) dir="$2"; shift 2;; *) shift;; esac; done
    cp -a "$FIXTURES"/. "$dir"/ ;;
  "release delete-asset") echo "DELETE \${2}" >> "$LOG" ;;
  "release upload")
    shift
    for path in "$@"; do
      case "$path" in --*) break ;; *) echo "UPLOAD $(basename "$path")" >> "$LOG" ;;
      esac
    done ;;
  *) echo "unstubbed gh call: $cmd $*" >&2; exit 2 ;;
esac
`

const stepScript = () => {
  const lines = readFileSync(WORKFLOW, 'utf8').split('\n')
  const named = lines.findIndex((line) => line.includes('name: Rename the release assets'))
  const runAt = lines.findIndex((line, index) => index > named && line.trim() === 'run: |')
  expect(named).toBeGreaterThan(-1)
  expect(runAt).toBeGreaterThan(named)

  const indent = lines[runAt].search(/\S/)
  let end = runAt + 1
  while (end < lines.length && (lines[end].trim() === '' || lines[end].search(/\S/) > indent)) end += 1

  return lines.slice(runAt + 1, end).join('\n')
}

const run = (released = RELEASED) => {
  const root = mkdtempSync(join(tmpdir(), 'stable-assets-'))
  const fixtures = join(root, 'fixtures')
  const bin = join(root, 'bin')
  const runnerTemp = join(root, 'runner')
  for (const dir of [fixtures, bin, runnerTemp]) mkdirSync(dir, { recursive: true })
  for (const name of released) writeFileSync(join(fixtures, name), 'x')

  writeFileSync(join(root, 'step.sh'), `#!/usr/bin/env bash\n${stepScript()}\n`)
  writeFileSync(join(bin, 'gh'), GH_STUB)
  chmodSync(join(bin, 'gh'), 0o755)
  const log = join(root, 'gh.log')
  writeFileSync(log, '')

  execFileSync('bash', [join(root, 'step.sh')], {
    env: {
      ...process.env,
      PATH: `${bin}:${process.env.PATH}`,
      RUNNER_TEMP: runnerTemp,
      GITHUB_REF_NAME: 'v0.1.2',
      GITHUB_REPOSITORY: 'EPFL-ENAC/enac-openwork-installer',
      FIXTURES: fixtures,
      LOG: log,
    },
    stdio: 'pipe',
  })

  return readFileSync(log, 'utf8').trim().split('\n').sort()
}

describe('the release asset consolidation step', () => {
  it('removes every versioned asset and uploads the four stable names', () => {
    const versioned = RELEASED.filter(
      (name) => !STABLE.includes(name) && !KEPT.includes(name),
    )
    const expected = [
      ...versioned.map((name) => `DELETE ${name}`),
      ...STABLE.map((name) => `UPLOAD ${name}`),
    ].sort()

    expect(run()).toEqual(expected)
  })

  it('fails instead of guessing when a platform asset is missing', () => {
    const partial = RELEASED.filter((name) => !name.includes('aarch64'))

    expect(() => run(partial)).toThrow()
  })
})
