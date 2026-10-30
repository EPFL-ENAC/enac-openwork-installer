import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen }))

import {
  installOpencode,
  installOpenwork,
  launchOpenwork,
  opencodeStatus,
  onPortalLogin,
  openworkStatus,
  portalClose,
  portalCreateKey,
  portalGetForm,
  portalStartLogin,
  smokeTest,
} from './ipc'

beforeEach(() => {
  invoke.mockReset()
  invoke.mockResolvedValue(undefined)
})

describe('ipc wrappers', () => {
  it('maps every command to its Rust name', async () => {
    await opencodeStatus()
    await installOpencode()
    await openworkStatus()
    await installOpenwork()
    await launchOpenwork()
    await smokeTest()
    await portalStartLogin()
    await portalGetForm()
    await portalCreateKey('Alice')
    await portalClose()

    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      'opencode_status',
      'install_opencode',
      'openwork_status',
      'install_openwork',
      'launch_openwork',
      'smoke_test',
      'portal_start_login',
      'portal_get_form',
      'portal_create_key',
      'portal_close',
    ])
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
  })
})
