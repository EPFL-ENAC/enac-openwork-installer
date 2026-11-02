import { beforeEach, describe, expect, it, vi } from 'vitest'

const ipc = vi.hoisted(() => ({
  opencodeStatus: vi.fn(),
  installOpencode: vi.fn(),
  openworkStatus: vi.fn(),
  installOpenwork: vi.fn(),
  configureOpenwork: vi.fn(),
  openworkStateStatus: vi.fn(),
  smokeTest: vi.fn(),
  onBackendLog: vi.fn(),
  onPortalLogin: vi.fn(),
  portalStartLogin: vi.fn(),
  portalGetForm: vi.fn(),
  portalCreateKey: vi.fn(),
  portalClose: vi.fn(),
  launchOpenwork: vi.fn(),
}))

const provider = vi.hoisted(() => ({ writeProviderConfig: vi.fn() }))
const secrets = vi.hoisted(() => ({ registerSecret: vi.fn() }))

vi.mock('./ipc', () => ipc)
vi.mock('./provider-config', () => provider)
vi.mock('./trace', () => secrets)

import type { FlowResults, StepId } from './flow'
import type { StepRun } from './orchestrator'
import { createRunners, type KeyAnswer, type KeyPrompt, type RunnerBridge } from './runners'

const TOKEN = 'rcp-token-0123456789'
const CONFIG_PATH = '~/.config/opencode/opencode.json'

const asker = (answer: KeyAnswer | ((prompt: KeyPrompt) => Promise<KeyAnswer>) = {}) =>
  vi.fn<(prompt: KeyPrompt) => Promise<KeyAnswer>>(async (prompt) =>
    typeof answer === 'function' ? answer(prompt) : answer,
  )

const bridge = (over: Partial<RunnerBridge> = {}): RunnerBridge => ({
  results: () => ({}),
  askKeys: vi.fn(async () => ({})),
  ...over,
})

const withKey = (): FlowResults => ({ rcpKey: { status: 'passed', token: TOKEN, keyName: 'OpenWork' } })

const runners = (over: Partial<RunnerBridge> = {}): StepRun[] => createRunners(bridge(over))

const step = (list: StepRun[], id: StepId): StepRun => {
  const found = list.find((entry) => entry.id === id)
  if (!found) throw new Error(`no runner for ${id}`)
  return found
}

const run = async (list: StepRun[], id: StepId) => {
  const lines: string[] = []
  const outcome = await step(list, id).run({ emit: (line) => lines.push(line) })
  return { outcome, lines }
}

const noBackendLog = () => ipc.onBackendLog.mockResolvedValue(() => {})
const noPortalEvent = () => ipc.onPortalLogin.mockResolvedValue(() => {})

beforeEach(() => {
  vi.clearAllMocks()
  noBackendLog()
  noPortalEvent()
  ipc.openworkStateStatus.mockResolvedValue({ broken: false, databases: [] })
  ipc.configureOpenwork.mockResolvedValue({
    configDir: '~/.config/openwork',
    providerWritten: true,
    envWritten: true,
    analyticsWritten: true,
    workspaceConfigWritten: true,
    defaultModelWorkspaceId: 'ws_05771fd9f545',
    defaultModelWritten: true,
  })
  ipc.opencodeStatus.mockResolvedValue({ installed: true, version: '1.2.3', path: '/usr/bin/opencode' })
  ipc.smokeTest.mockResolvedValue({ ok: true, detail: 'pong', error: null })
  ipc.portalClose.mockResolvedValue(undefined)
  provider.writeProviderConfig.mockResolvedValue({
    ok: true,
    configPath: CONFIG_PATH,
    backupPath: `${CONFIG_PATH}.bak`,
    changed: true,
    backupCreated: false,
  })
})

describe('runner order', () => {
  it('runs the RCP key before the merged OpenCode step', () => {
    expect(runners().map((entry) => entry.id)).toEqual(['network', 'rcpKey', 'opencode', 'openwork'])
  })
})

describe('network runner', () => {
  it('passes when the portal answers', async () => {
    const fetchMock = vi.fn().mockResolvedValue({})
    vi.stubGlobal('fetch', fetchMock)

    const { outcome, lines } = await run(runners(), 'network')

    expect(outcome.status).toBe('passed')
    expect(fetchMock).toHaveBeenCalledWith(
      'https://portal.rcp.epfl.ch',
      expect.objectContaining({ mode: 'no-cors' }),
    )
    expect(lines.join('\n')).toContain('reachable')
    vi.unstubAllGlobals()
  })

  it('fails with the network error kind when the fetch throws', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')))

    const { outcome, lines } = await run(runners(), 'network')

    expect(outcome).toEqual({ status: 'failed', error: { kind: 'network', message: expect.any(String) } })
    expect(lines.join('\n')).toContain('not reachable')
    vi.unstubAllGlobals()
  })
})

describe('merged opencode runner', () => {
  it('skips the install sub-action, then writes the config and runs the smoke test', async () => {
    const { outcome, lines } = await run(runners({ results: withKey }), 'opencode')

    expect(ipc.installOpencode).not.toHaveBeenCalled()
    expect(provider.writeProviderConfig).toHaveBeenCalledWith(TOKEN)
    expect(ipc.smokeTest).toHaveBeenCalledWith(TOKEN)
    expect(outcome.status).toBe('passed')
    expect(outcome).toMatchObject({ result: { version: '1.2.3', path: CONFIG_PATH, ok: true } })

    const text = lines.join('\n')
    expect(text).toContain('already installed, skip')
    expect(text).toContain(`The configuration is written to ${CONFIG_PATH}`)
    expect(text).toContain('The smoke test passed')
    expect(text).not.toContain(TOKEN)
  })

  it('installs a missing OpenCode and forwards only its backend-log lines', async () => {
    ipc.opencodeStatus.mockResolvedValue({ installed: false, version: null, path: null })
    ipc.onBackendLog.mockImplementation(async (handler: (log: { task: string; message: string }) => void) => {
      handler({ task: 'opencode', message: 'downloading' })
      handler({ task: 'openwork', message: 'other task' })
      return () => {}
    })
    ipc.installOpencode.mockResolvedValue({ version: '1.2.3', path: '/usr/local/bin/opencode' })

    const { outcome, lines } = await run(runners({ results: withKey }), 'opencode')

    expect(outcome).toEqual({
      status: 'passed',
      result: { version: '1.2.3', path: CONFIG_PATH, ok: true, detail: 'pong' },
    })
    expect(lines).toContain('downloading')
    expect(lines).not.toContain('other task')
    expect(lines.join('\n')).toContain('OpenCode 1.2.3 is installed')
  })

  it('stops before the config write when the install sub-action fails', async () => {
    ipc.opencodeStatus.mockResolvedValue({ installed: false, version: null, path: null })
    ipc.installOpencode.mockRejectedValue({ kind: 'install', message: 'no disk' })

    const { outcome, lines } = await run(runners({ results: withKey }), 'opencode')

    expect(outcome).toEqual({ status: 'failed', error: { kind: 'install', message: 'no disk' } })
    expect(provider.writeProviderConfig).not.toHaveBeenCalled()
    expect(ipc.smokeTest).not.toHaveBeenCalled()
    expect(lines.join('\n')).toContain('OpenCode installation failed')
  })

  it('stops before the smoke test when the config write fails and names the sub-action', async () => {
    provider.writeProviderConfig.mockResolvedValue({ ok: false, code: 'io', detail: 'disk full' })

    const { outcome, lines } = await run(runners({ results: withKey }), 'opencode')

    expect(outcome).toEqual({ status: 'failed', error: { kind: 'io', message: 'disk full' } })
    expect(ipc.smokeTest).not.toHaveBeenCalled()
    expect(lines.join('\n')).toContain('The OpenCode configuration was not written: disk full')
  })

  it('fails with the smoke error kind and names the sub-action', async () => {
    ipc.smokeTest.mockResolvedValue({ ok: false, detail: null, error: 'HTTP 401' })

    const { outcome, lines } = await run(runners({ results: withKey }), 'opencode')

    expect(outcome).toEqual({ status: 'failed', error: { kind: 'smoke', message: 'HTTP 401' } })
    expect(lines.join('\n')).toContain('The smoke test failed: HTTP 401')
  })

  it('fails without a key from the previous step', async () => {
    const { outcome, lines } = await run(runners(), 'opencode')

    expect(outcome).toEqual({
      status: 'failed',
      error: { kind: 'missing-key', message: expect.any(String) },
    })
    expect(provider.writeProviderConfig).not.toHaveBeenCalled()
    expect(ipc.smokeTest).not.toHaveBeenCalled()
    expect(lines.join('\n')).toContain('No RCP key is available')
  })
})

describe('openwork runner', () => {
  it('configures the provider after a skipped install and reports the config dir', async () => {
    ipc.openworkStatus.mockResolvedValue({ installed: true, version: '1.2.3', path: '/usr/bin/openwork' })

    const { outcome, lines } = await run(runners({ results: withKey }), 'openwork')

    expect(ipc.installOpenwork).not.toHaveBeenCalled()
    expect(ipc.configureOpenwork).toHaveBeenCalledWith(TOKEN)
    expect(outcome).toEqual({
      status: 'passed',
      result: { version: '1.2.3', path: '/usr/bin/openwork', configDir: '~/.config/openwork' },
    })
    const text = lines.join('\n')
    expect(text).toContain('already installed')
    expect(text).toContain('The epfl-aiaas provider is written into the OpenWork configuration')
    expect(text).toContain('stored in the OpenWork environment store')
    expect(text).toContain('The OpenWork anonymous usage data is turned off')
    expect(text).toContain('The default model is set to epfl-aiaas/large')
    expect(text).not.toContain(TOKEN)
  })

  it('configures after a fresh install', async () => {
    ipc.openworkStatus.mockResolvedValue({ installed: false, version: null, path: null })
    ipc.installOpenwork.mockResolvedValue({ version: '0.18.63', path: '/opt/openwork' })

    const { outcome, lines } = await run(runners({ results: withKey }), 'openwork')

    expect(ipc.installOpenwork).toHaveBeenCalled()
    expect(ipc.configureOpenwork).toHaveBeenCalledWith(TOKEN)
    expect(outcome).toEqual({
      status: 'passed',
      result: { version: '0.18.63', path: '/opt/openwork', configDir: '~/.config/openwork' },
    })
    expect(lines.join('\n')).toContain('OpenWork 0.18.63 is installed')
  })

  it('fails the step when the configuration fails', async () => {
    ipc.configureOpenwork.mockRejectedValue({ kind: 'io', message: 'env store locked' })

    const { outcome, lines } = await run(runners({ results: withKey }), 'openwork')

    expect(outcome).toEqual({ status: 'failed', error: { kind: 'io', message: 'env store locked' } })
    expect(lines.join('\n')).toContain('The OpenWork configuration failed')
  })

  it('fails without a key from the previous step', async () => {
    const { outcome } = await run(runners(), 'openwork')

    expect(outcome).toEqual({
      status: 'failed',
      error: { kind: 'missing-key', message: expect.any(String) },
    })
    expect(ipc.configureOpenwork).not.toHaveBeenCalled()
  })

  it('fails with the backend error kind when the status call fails', async () => {
    ipc.openworkStatus.mockRejectedValue({ kind: 'install', message: 'boom' })

    const { outcome } = await run(runners(), 'openwork')

    expect(outcome).toEqual({ status: 'failed', error: { kind: 'install', message: 'boom' } })
  })

  it('fails the openwork runner when the database blocks the start', async () => {
    ipc.openworkStateStatus.mockResolvedValue({ broken: true, databases: ['opencode.db'] })

    const { outcome, lines } = await run(runners(), 'openwork')

    expect(outcome).toEqual({
      status: 'failed',
      error: { kind: 'openwork-broken', message: 'opencode.db' },
    })
    expect(lines.join('\n')).toContain('opencode.db')
    expect(ipc.openworkStatus).not.toHaveBeenCalled()
  })
})

describe('rcpKey runner', () => {
  const loginOk = (form: { units: string[]; existingOpenWorkKey: boolean }) => {
    ipc.portalStartLogin.mockImplementation(async () => {
      const handler = ipc.onPortalLogin.mock.calls[0][0]
      handler({ ok: true, error: null })
    })
    ipc.portalGetForm.mockResolvedValue(form)
  }

  it('creates a key without a pause when one unit is offered', async () => {
    loginOk({ units: ['ENAC'], existingOpenWorkKey: false })
    ipc.portalCreateKey.mockResolvedValue({ name: 'OpenWork', token: TOKEN })
    const askKeys = asker()

    const { outcome, lines } = await run(runners({ askKeys }), 'rcpKey')

    expect(askKeys).not.toHaveBeenCalled()
    expect(ipc.portalCreateKey).toHaveBeenCalledWith('OpenWork', 'ENAC')
    expect(outcome).toEqual({
      status: 'passed',
      result: { keyName: 'OpenWork', token: TOKEN },
    })
    expect(secrets.registerSecret).toHaveBeenCalledWith(TOKEN)
    expect(lines.join('\n')).not.toContain(TOKEN)
  })

  it('pauses on the unit picker and creates the key with the chosen unit', async () => {
    loginOk({ units: ['ENAC', 'STI'], existingOpenWorkKey: false })
    ipc.portalCreateKey.mockResolvedValue({ name: 'MyKey', token: TOKEN })
    const askKeys = asker({ unit: 'STI', keyName: 'MyKey' })

    const { outcome } = await run(runners({ askKeys }), 'rcpKey')

    const prompt = askKeys.mock.calls[0][0]
    expect(prompt.showUnitPicker).toBe(true)
    expect(prompt.branch).toBe('fresh')
    expect(prompt.units).toEqual(['ENAC', 'STI'])
    expect(ipc.portalCreateKey).toHaveBeenCalledWith('MyKey', 'STI')
    expect(outcome.status).toBe('passed')
  })

  it('reuses an existing key without prompting', async () => {
    loginOk({ units: ['ENAC'], existingOpenWorkKey: true })
    ipc.portalCreateKey.mockResolvedValue({ name: 'OpenWork', token: TOKEN })
    const askKeys = asker()

    const { outcome, lines } = await run(runners({ askKeys }), 'rcpKey')

    expect(askKeys).not.toHaveBeenCalled()
    expect(ipc.portalCreateKey).toHaveBeenCalledWith('OpenWork', 'ENAC')
    expect(outcome).toEqual({
      status: 'passed',
      result: { keyName: 'OpenWork', token: TOKEN },
    })
    expect(secrets.registerSecret).toHaveBeenCalledWith(TOKEN)
    expect(lines.join('\n')).not.toContain(TOKEN)
  })

  it('waits for the answer before it continues', async () => {
    loginOk({ units: ['ENAC', 'STI'], existingOpenWorkKey: false })
    let answer: (value: KeyAnswer) => void = () => {}
    const askKeys = vi.fn<(prompt: KeyPrompt) => Promise<KeyAnswer>>(
      () =>
        new Promise<KeyAnswer>((resolve) => {
          answer = resolve
        }),
    )

    const pending = run(runners({ askKeys }), 'rcpKey')
    let settled = false
    void pending.then(() => {
      settled = true
    })
    await new Promise((resolve) => setTimeout(resolve, 5))
    expect(settled).toBe(false)

    answer({ token: TOKEN })
    const { outcome } = await pending
    expect(outcome.status).toBe('passed')
  })

  it('asks again with a new name when the portal refuses the key', async () => {
    loginOk({ units: ['ENAC'], existingOpenWorkKey: false })
    ipc.portalCreateKey
      .mockRejectedValueOnce({ kind: 'portal-refused', message: 'name taken' })
      .mockResolvedValueOnce({ name: 'OpenWork-2', token: TOKEN })
    const askKeys = asker({ keyName: 'OpenWork-2' })

    const { outcome } = await run(runners({ askKeys }), 'rcpKey')

    expect(askKeys).toHaveBeenCalledTimes(1)
    const prompt = askKeys.mock.calls[0][0]
    expect(prompt.branch).toBe('refused')
    expect(ipc.portalCreateKey).toHaveBeenLastCalledWith('OpenWork-2', 'ENAC')
    expect(outcome).toEqual({
      status: 'passed',
      result: { keyName: 'OpenWork-2', token: TOKEN },
    })
  })

  it('pauses on the manual fallback when the portal login fails', async () => {
    ipc.portalStartLogin.mockImplementation(async () => {
      const handler = ipc.onPortalLogin.mock.calls[0][0]
      handler({ ok: false, error: 'cancelled' })
    })
    const askKeys = asker({ token: TOKEN })

    const { outcome } = await run(runners({ askKeys }), 'rcpKey')

    const prompt = askKeys.mock.calls[0][0]
    expect(prompt.manual).toBe(true)
    expect(outcome).toEqual({
      status: 'passed',
      result: { keyName: 'OpenWork', token: TOKEN },
    })
    expect(ipc.portalGetForm).not.toHaveBeenCalled()
  })

  it('fails when the manual fallback gives no token', async () => {
    ipc.portalStartLogin.mockRejectedValue({ kind: 'portal', message: 'no window' })

    const { outcome } = await run(runners(), 'rcpKey')

    expect(outcome).toEqual({
      status: 'failed',
      error: { kind: 'portal-auth', message: expect.any(String) },
    })
  })
})
