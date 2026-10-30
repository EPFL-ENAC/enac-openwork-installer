import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export interface InstallStatus {
  installed: boolean
  version: string | null
  path: string | null
}

export interface InstallResult {
  version: string
  path: string
}

export interface SmokeResult {
  ok: boolean
  detail: string | null
  error: string | null
}

export interface PortalForm {
  units: string[]
  existingOpenWorkKey: boolean
}

export interface CreatedKey {
  name: string
  token: string
}

export interface AppError {
  kind: string
  message: string
}

export interface BackendLog {
  task: string
  message: string
}

export interface OpenWorkState {
  broken: boolean
  databases: string[]
}

export interface StateRepair {
  backups: string[]
}

export interface PortalLogin {
  ok: boolean
  error: string | null
}

export const opencodeStatus = () => invoke<InstallStatus>('opencode_status')
export const installOpencode = () => invoke<InstallResult>('install_opencode')
export const openworkStatus = () => invoke<InstallStatus>('openwork_status')
export const installOpenwork = () => invoke<InstallResult>('install_openwork')
export const launchOpenwork = () => invoke<void>('launch_openwork')
export const openworkStateStatus = () => invoke<OpenWorkState>('openwork_state_status')
export const openworkRepairState = () => invoke<StateRepair>('openwork_repair_state')
export const smokeTest = () => invoke<SmokeResult>('smoke_test')
export const portalStartLogin = () => invoke<void>('portal_start_login')
export const portalGetForm = () => invoke<PortalForm>('portal_get_form')
export const portalCreateKey = (name: string, unit?: string | null) =>
  invoke<CreatedKey>('portal_create_key', { name, unit: unit ?? null })
export const portalClose = () => invoke<void>('portal_close')
export const onPortalLogin = (handler: (result: PortalLogin) => void): Promise<UnlistenFn> =>
  listen<PortalLogin>('portal-login', (event) => handler(event.payload))
