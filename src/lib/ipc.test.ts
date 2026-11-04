import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen }))

import {
  installOpenwork,
  launchOpenwork,
  onPortalLogin,
  openworkStatus,
  portalClose,
  portalCreateKey,
  portalGetForm,
  portalListKeys,
  portalStartLogin,
  portalUseKey,
  quitApp,
  restrictProviderConfig,
  smokeTest,
} from './ipc'

beforeEach(() => {
  invoke.mockReset()
  invoke.mockResolvedValue(undefined)
})

describe('ipc wrappers', () => {
  it('maps every command to its Rust name', async () => {
    await openworkStatus()
    await installOpenwork()
    await launchOpenwork()
    await smokeTest()
    await portalStartLogin()
    await portalGetForm()
    await portalCreateKey('Alice')
    await portalListKeys()
    await portalUseKey(1462)
    await portalClose()
    await restrictProviderConfig()
    await quitApp()

    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      'openwork_status',
      'install_openwork',
      'launch_openwork',
      'smoke_test',
      'portal_start_login',
      'portal_get_form',
      'portal_create_key',
      'portal_list_keys',
      'portal_use_key',
      'portal_close',
      'restrict_provider_config',
      'quit_app',
    ])
  })

  it('carries the engine choice and the token to the smoke command', async () => {
    await smokeTest('tok', true)

    expect(invoke).toHaveBeenLastCalledWith('smoke_test', { token: 'tok', freshInstall: true })

    await smokeTest()

    expect(invoke).toHaveBeenLastCalledWith('smoke_test', { token: null, freshInstall: false })
  })

  it('forwards the portal-login event payload', async () => {
    const unlisten = vi.fn()
    listen.mockResolvedValue(unlisten)
    const handler = vi.fn()

    const result = await onPortalLogin(handler)
    const [, onEvent] = listen.mock.calls[0]
    onEvent({ payload: { ok: true, error: null } })

    expect(listen).toHaveBeenCalledWith('portal-login', expect.any(Function))
    expect(handler).toHaveBeenCalledWith({ ok: true, error: null })
    expect(result).toBe(unlisten)
  })

  it('sends the portal key arguments', async () => {
    await portalCreateKey('Alice', 'ENAC')
    expect(invoke).toHaveBeenLastCalledWith('portal_create_key', { name: 'Alice', unit: 'ENAC' })

    await portalCreateKey('Bob')
    expect(invoke).toHaveBeenLastCalledWith('portal_create_key', { name: 'Bob', unit: null })

    await portalUseKey(834)
    expect(invoke).toHaveBeenLastCalledWith('portal_use_key', { id: 834 })
  })
})
