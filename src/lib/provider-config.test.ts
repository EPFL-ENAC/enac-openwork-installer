import { beforeEach, describe, expect, it, vi } from 'vitest'

const fs = vi.hoisted(() => ({
  exists: vi.fn(),
  readTextFile: vi.fn(),
  writeTextFile: vi.fn(),
  mkdir: vi.fn(),
  copyFile: vi.fn(),
}))

vi.mock('@tauri-apps/plugin-fs', () => ({ BaseDirectory: { Home: 21 }, ...fs }))

import {
  BACKUP_PATH,
  CONFIG_PATH,
  PROVIDER_ID,
  RCP_BASE_URL,
  displayPath,
  mergeProviderConfig,
  writeProviderConfig,
} from './provider-config'

type Config = Record<string, any>

const API_KEY = 'sk-test-key'

const config = (): Config => mergeProviderConfig(null, API_KEY) as Config
const epfl = (value: Config) => value.providers[PROVIDER_ID]

beforeEach(() => {
  fs.exists.mockReset().mockResolvedValue(false)
  fs.readTextFile.mockReset().mockResolvedValue('')
  fs.writeTextFile.mockReset().mockResolvedValue(undefined)
  fs.mkdir.mockReset().mockResolvedValue(undefined)
  fs.copyFile.mockReset().mockResolvedValue(undefined)
})

describe('mergeProviderConfig', () => {
  it('creates the RCP provider in an empty config', () => {
    const merged = config()

    expect(merged.$schema).toBe('https://opencode.ai/config.json')
    expect(Object.keys(merged.providers)).toEqual([PROVIDER_ID])
    expect(epfl(merged)).toMatchObject({
      name: 'RCP AI Inference as a Service',
      package: 'aisdk:@ai-sdk/openai-compatible',
      settings: { baseURL: RCP_BASE_URL, apiKey: API_KEY },
    })
    expect(Object.keys(epfl(merged).models)).toEqual(['epfl-aiaas/large', 'epfl-aiaas/small'])
  })

  it('keeps other providers and user settings', () => {
    const existing = {
      theme: 'dark',
      model: 'anthropic/claude',
      providers: { anthropic: { name: 'Anthropic', options: { timeout: 30 } } },
    }

    const merged = mergeProviderConfig(existing, API_KEY) as Config

    expect(merged.theme).toBe('dark')
    expect(merged.model).toBe('anthropic/claude')
    expect(merged.providers.anthropic).toEqual({ name: 'Anthropic', options: { timeout: 30 } })
    expect(epfl(merged).settings.baseURL).toBe(RCP_BASE_URL)
  })

  it('updates an existing RCP entry in place and keeps the rest', () => {
    const existing = {
      providers: {
        'epfl-rcp-aiaas': { name: 'Stale', package: 'stale', models: { old: {} } },
        local: { name: 'Local' },
      },
    }

    const merged = mergeProviderConfig(existing, API_KEY) as Config

    expect(epfl(merged).name).toBe('RCP AI Inference as a Service')
    expect(epfl(merged).models.old).toBeUndefined()
    expect(merged.providers.local).toEqual({ name: 'Local' })
    expect(Object.keys(merged.providers)).toEqual([PROVIDER_ID, 'local'])
  })

  it('is idempotent on re-run', () => {
    const first = mergeProviderConfig(null, API_KEY)
    const second = mergeProviderConfig(first, API_KEY)

    expect(second).toEqual(first)
    expect(JSON.stringify(second)).toBe(JSON.stringify(first))
  })

  it('does not mutate its argument', () => {
    const existing = { providers: { local: { name: 'Local' } } }
    const before = JSON.stringify(existing)

    mergeProviderConfig(existing, API_KEY)

    expect(JSON.stringify(existing)).toBe(before)
  })

  it('writes the API key into the RCP provider settings', () => {
    expect(epfl(config()).settings.apiKey).toBe(API_KEY)
  })

  it('replaces a non-object providers value and keeps sibling keys', () => {
    const merged = mergeProviderConfig({ providers: ['broken'], autoupdate: false }, API_KEY) as Config

    expect(merged.autoupdate).toBe(false)
    expect(epfl(merged).package).toBe('aisdk:@ai-sdk/openai-compatible')
  })

  it('keeps a user provided $schema', () => {
    expect(mergeProviderConfig({ $schema: 'custom' }, API_KEY)).toMatchObject({ $schema: 'custom' })
  })
})

describe('displayPath', () => {
  it('shows home shorthand on unix and USERPROFILE on windows', () => {
    expect(displayPath(CONFIG_PATH, 'Mozilla/5.0 (X11; Linux x86_64)')).toBe(
      `~/${CONFIG_PATH}`,
    )
    expect(displayPath(CONFIG_PATH, 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)')).toBe(
      '%USERPROFILE%\\.config\\opencode\\opencode.json',
    )
  })
})

describe('writeProviderConfig', () => {
  it('creates the config file without a backup', async () => {
    const outcome = await writeProviderConfig(API_KEY)

    expect(outcome).toMatchObject({ ok: true, backupCreated: false, changed: true })
    expect(fs.copyFile).not.toHaveBeenCalled()
    expect(fs.mkdir).toHaveBeenCalled()
    expect(fs.writeTextFile).toHaveBeenCalledWith(
      CONFIG_PATH,
      expect.stringContaining('"epfl-rcp-aiaas"'),
      expect.anything(),
    )
  })

  it('backups an existing config before the first write', async () => {
    fs.exists.mockImplementation(async (path: string) => path === CONFIG_PATH)
    fs.readTextFile.mockResolvedValue(JSON.stringify({ theme: 'dark' }))

    const outcome = await writeProviderConfig(API_KEY)

    expect(outcome).toMatchObject({ ok: true, backupCreated: true, changed: true })
    expect(fs.copyFile).toHaveBeenCalledWith(CONFIG_PATH, BACKUP_PATH, expect.anything())
    expect(fs.writeTextFile).toHaveBeenCalledOnce()
  })

  it('keeps an existing backup instead of overwriting it', async () => {
    fs.exists.mockImplementation(async (path: string) => [CONFIG_PATH, BACKUP_PATH].includes(path))
    fs.readTextFile.mockResolvedValue(JSON.stringify({ theme: 'dark' }))

    const outcome = await writeProviderConfig(API_KEY)

    expect(outcome).toMatchObject({ ok: true, backupCreated: false, changed: true })
    expect(fs.copyFile).not.toHaveBeenCalled()
  })

  it('writes nothing when the config is already merged', async () => {
    const merged = mergeProviderConfig({ theme: 'dark' }, API_KEY)
    fs.exists.mockResolvedValue(true)
    fs.readTextFile.mockResolvedValue(JSON.stringify(merged))

    const outcome = await writeProviderConfig(API_KEY)

    expect(outcome).toMatchObject({ ok: true, changed: false, backupCreated: false })
    expect(fs.copyFile).not.toHaveBeenCalled()
    expect(fs.writeTextFile).not.toHaveBeenCalled()
  })

  it('never overwrites an unparsable config', async () => {
    fs.exists.mockResolvedValue(true)
    fs.readTextFile.mockResolvedValue('{ broken')

    const outcome = await writeProviderConfig(API_KEY)

    expect(outcome).toMatchObject({ ok: false, code: 'invalid-json' })
    expect(fs.copyFile).not.toHaveBeenCalled()
    expect(fs.writeTextFile).not.toHaveBeenCalled()
  })

  it('reports a read failure as an io error', async () => {
    fs.exists.mockResolvedValue(true)
    fs.readTextFile.mockRejectedValue(new Error('forbidden path'))

    const outcome = await writeProviderConfig(API_KEY)

    expect(outcome).toMatchObject({ ok: false, code: 'io' })
    expect(fs.writeTextFile).not.toHaveBeenCalled()
  })
})

describe('config paths', () => {
  it('targets the global OpenCode config and its backup', () => {
    expect(CONFIG_PATH).toBe('.config/opencode/opencode.json')
    expect(BACKUP_PATH).toBe('.config/opencode/opencode.json.bak')
    expect(RCP_BASE_URL).toBe('https://inference-rcp.epfl.ch/v1')
  })
})
