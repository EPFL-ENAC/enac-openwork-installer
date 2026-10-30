import { beforeEach, describe, expect, it, vi } from 'vitest'

const ipc = vi.hoisted(() => ({
  opencodeStatus: vi.fn(),
  installOpencode: vi.fn(),
  openworkStatus: vi.fn(),
  installOpenwork: vi.fn(),
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

const asker = (answer: KeyAnswer | ((prompt: KeyPrompt) => Promise<KeyAnswer>) = {}) =>
  vi.fn<(prompt: KeyPrompt) => Promise<KeyAnswer>>(async (prompt) =>
    typeof answer === 'function' ? answer(prompt) : answer,
  )

const bridge = (over: Partial<RunnerBridge> = {}): RunnerBridge => ({
  results: () => ({}),
  askKeys: vi.fn(async () => ({})),
  ...over,
})

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
  ipc.portalClose.mockResolvedValue(undefined)
  provider.writeProviderConfig.mockResolvedValue({
    ok: true,
    configPath: '~/.config/opencode/opencode.json',
    backupPath: '~/.config/opencode/opencode.json.bak',
    changed: true,
    backupCreated: false,
  })
})

describe('runner order', () => {
  it('runs the four auto steps between network and openwork', () => {
    expect(runners().map((entry) => entry.id)).toEqual([
      'network',
      'opencode',
      'rcpKey',
      'providerConfig',
      'smokeTest',
      'openwork',
    ])
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

describe('tool runners', () => {
  it('skips an installed tool and logs a skip line', async () => {
    ipc.opencodeStatus.mockResolvedValue({ installed: true, version: '1.2.3', path: '/usr/bin/opencode' })

    const { outcome, lines } = await run(runners(), 'opencode')

    expect(outcome).toEqual({
      status: 'skipped',
      result: { version: '1.2.3', path: '/usr/bin/opencode' },
    })
    expect(ipc.installOpencode).not.toHaveBeenCalled()
    expect(lines.join('\n')).toContain('already installed')
  })

  it('installs a missing tool and forwards only its backend-log lines', async () => {
    ipc.opencodeStatus.mockResolvedValue({ installed: false, version: null, path: null })
    ipc.onBackendLog.mockImplementation(async (handler: (log: { task: string; message: string }) => void) => {
      handler({ task: 'opencode', message: 'downloading' })
      handler({ task: 'openwork', message: 'other task' })
      return () => {}
    })
    ipc.installOpencode.mockResolvedValue({ version: '1.2.3', path: '/usr/local/bin/opencode' })

    const { outcome, lines } = await run(runners(), 'opencode')

    expect(outcome).toEqual({
      status: 'passed',
      result: { version: '1.2.3', path: '/usr/local/bin/opencode' },
    })
    expect(lines).toContain('downloading')
    expect(lines).not.toContain('other task')
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

describe('provider config runner', () => {
  it('writes the config with the key from the rcpKey step', async () => {
    const results = (): FlowResults => ({ rcpKey: { status: 'passed', token: TOKEN, keyName: 'OpenWork' } })

    const { outcome, lines } = await run(runners({ results }), 'providerConfig')

    expect(provider.writeProviderConfig).toHaveBeenCalledWith(TOKEN)
    expect(outcome).toEqual({
      status: 'passed',
      result: { path: '~/.config/opencode/opencode.json' },
    })
    expect(lines.join('\n')).not.toContain(TOKEN)
  })

  it('fails when the writer reports an error', async () => {
    const results = (): FlowResults => ({ rcpKey: { status: 'passed', token: TOKEN } })
    provider.writeProviderConfig.mockResolvedValue({ ok: false, code: 'io', detail: 'disk full' })

    const { outcome } = await run(runners({ results }), 'providerConfig')

    expect(outcome).toEqual({ status: 'failed', error: { kind: 'io', message: 'disk full' } })
  })

  it('fails without a key from the previous step', async () => {
    const { outcome } = await run(runners(), 'providerConfig')

    expect(outcome).toEqual({
      status: 'failed',
      error: { kind: 'missing-key', message: expect.any(String) },
    })
    expect(provider.writeProviderConfig).not.toHaveBeenCalled()
  })
})

describe('smoke test runner', () => {
  it('passes when the smoke test succeeds', async () => {
    ipc.smokeTest.mockResolvedValue({ ok: true, detail: 'pong', error: null })

    const { outcome } = await run(runners(), 'smokeTest')

    expect(outcome).toEqual({ status: 'passed', result: { ok: true, detail: 'pong' } })
  })

  it('fails with the smoke error kind when the smoke test reports an error', async () => {
    ipc.smokeTest.mockResolvedValue({ ok: false, detail: null, error: 'HTTP 401' })

    const { outcome } = await run(runners(), 'smokeTest')

    expect(outcome).toEqual({ status: 'failed', error: { kind: 'smoke', message: 'HTTP 401' } })
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

  it('pauses on key choice and uses a pasted existing token', async () => {
    loginOk({ units: ['ENAC'], existingOpenWorkKey: true })
    const askKeys = asker({ token: TOKEN })

    const { outcome, lines } = await run(runners({ askKeys }), 'rcpKey')

    const prompt = askKeys.mock.calls[0][0]
    expect(prompt.branch).toBe('existing')
    expect(prompt.suggestedKeyName).toBe('OpenWork-2')
    expect(ipc.portalCreateKey).not.toHaveBeenCalled()
    expect(outcome).toEqual({
      status: 'passed',
      result: { keyName: 'OpenWork-2', token: TOKEN },
    })
    expect(secrets.registerSecret).toHaveBeenCalledWith(TOKEN)
    expect(lines.join('\n')).not.toContain(TOKEN)
  })

  it('waits for the answer before it continues', async () => {
    loginOk({ units: ['ENAC'], existingOpenWorkKey: true })
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
