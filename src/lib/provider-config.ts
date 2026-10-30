import { BaseDirectory, copyFile, exists, mkdir, readTextFile, writeTextFile } from '@tauri-apps/plugin-fs'

export const PROVIDER_ID = 'epfl-rcp-aiaas'
export const RCP_BASE_URL = 'https://inference-rcp.epfl.ch/v1'
export const CONFIG_SCHEMA = 'https://opencode.ai/config.json'

export const CONFIG_DIR = '.config/opencode'
export const CONFIG_PATH = `${CONFIG_DIR}/opencode.json`
export const BACKUP_PATH = `${CONFIG_PATH}.bak`

const BASE_DIR = BaseDirectory.Home

export type JsonObject = Record<string, unknown>

// Local copy of the RCP wiki reference page https://wiki.rcp.epfl.ch/home/AIaaS/compatible-tools,
// OpenCode v2 example. Re-check it when RCP publishes new models or prices.
// No apiKey: the wiki keeps the key out of opencode.json.
export const rcpProvider = (): JsonObject => ({
  name: 'RCP AI Inference as a Service',
  package: 'aisdk:@ai-sdk/openai-compatible',
  settings: { baseURL: RCP_BASE_URL },
  models: {
    'epfl-aiaas/large': {
      name: 'RCP AIaaS Large',
      modelID: 'epfl-aiaas/large',
      capabilities: { input: ['image', 'text'], output: ['text'] },
      cost: { input: 0.1664, output: 0.4991, cache: { read: 0.0166 } },
      settings: {},
    },
    'epfl-aiaas/small': {
      name: 'RCP AIaaS Small',
      modelID: 'epfl-aiaas/small',
      capabilities: { input: ['image', 'text'], output: ['text'] },
      cost: { input: 0.1671, output: 0.5014, cache: { read: 0.0167 } },
      settings: { reasoningEffort: 'none' },
      variants: [
        { id: 'none', settings: { reasoningEffort: 'none' } },
        { id: 'low', settings: { reasoningEffort: 'low' } },
        { id: 'high', settings: { reasoningEffort: 'high' } },
        { id: 'xhigh', settings: { reasoningEffort: 'xhigh' } },
        { id: 'max', settings: { reasoningEffort: 'max' } },
      ],
    },
  },
})

const isObject = (value: unknown): value is JsonObject =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T

export const displayPath = (path: string, platform = currentPlatform()): string =>
  /windows/i.test(platform) ? `%USERPROFILE%\\${path.replaceAll('/', '\\')}` : `~/${path}`

export function currentPlatform(): string {
  return typeof navigator === 'undefined' ? '' : navigator.userAgent
}

/**
 * Returns a copy of `existing` with the RCP provider added or updated in place.
 * Other providers, user settings and an existing `$schema` are kept untouched.
 */
export function mergeProviderConfig(existing: object | null): JsonObject {
  const base = isObject(existing) ? clone(existing) : {}
  const providers = isObject(base.providers) ? base.providers : {}
  providers[PROVIDER_ID] = rcpProvider()
  return { $schema: CONFIG_SCHEMA, ...base, providers }
}

export type ProviderConfigErrorCode = 'invalid-json' | 'io'

export type WriteOutcome =
  | { ok: true; configPath: string; backupPath: string; changed: boolean; backupCreated: boolean }
  | { ok: false; code: ProviderConfigErrorCode; detail: string }

/** Reads, merges, backs up (once) and writes the global OpenCode config. */
export async function writeProviderConfig(): Promise<WriteOutcome> {
  const paths = {
    configPath: displayPath(CONFIG_PATH),
    backupPath: displayPath(BACKUP_PATH),
  }
  const options = { baseDir: BASE_DIR }

  try {
    const hasConfig = await exists(CONFIG_PATH, options)
    const raw = hasConfig ? await readTextFile(CONFIG_PATH, options) : null

    let previous: JsonObject | null = null
    if (raw !== null) {
      let parsed: unknown
      try {
        parsed = JSON.parse(raw)
      } catch (error) {
        return { ok: false, code: 'invalid-json', detail: (error as Error).message }
      }
      if (!isObject(parsed)) {
        return { ok: false, code: 'invalid-json', detail: 'top-level JSON value is not an object' }
      }
      previous = parsed
    }

    const merged = mergeProviderConfig(previous)
    const changed = previous === null || JSON.stringify(previous) !== JSON.stringify(merged)
    if (!changed) return { ok: true, ...paths, changed: false, backupCreated: false }

    let backupCreated = false
    if (previous !== null && !(await exists(BACKUP_PATH, options))) {
      await copyFile(CONFIG_PATH, BACKUP_PATH, {
        fromPathBaseDir: BASE_DIR,
        toPathBaseDir: BASE_DIR,
      })
      backupCreated = true
    }

    if (!(await exists(CONFIG_DIR, options))) {
      await mkdir(CONFIG_DIR, { baseDir: BASE_DIR, recursive: true })
    }
    await writeTextFile(CONFIG_PATH, `${JSON.stringify(merged, null, 2)}\n`, options)

    return { ok: true, ...paths, changed: true, backupCreated }
  } catch (error) {
    return { ok: false, code: 'io', detail: (error as Error).message }
  }
}
