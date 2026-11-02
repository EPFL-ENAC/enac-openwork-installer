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

export interface PortalKeyChoice {
  id: number
  name: string
  pool: string
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

export interface ConfigureOpenworkOutcome {
  configDir: string
  providerWritten: boolean
  envWritten: boolean
  analyticsWritten: boolean
  workspaceConfigWritten: boolean
  defaultModelWorkspaceId: string | null
  defaultModelWritten: boolean
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
export const configureOpenwork = (token: string) =>
  invoke<ConfigureOpenworkOutcome>('configure_openwork', { token })
export const launchOpenwork = () => invoke<void>('launch_openwork')
export const openworkStateStatus = () => invoke<OpenWorkState>('openwork_state_status')
export const openworkRepairState = () => invoke<StateRepair>('openwork_repair_state')
export const smokeTest = (token?: string | null) =>
  invoke<SmokeResult>('smoke_test', { token: token ?? null })
export const portalStartLogin = () => invoke<void>('portal_start_login')
export const portalGetForm = () => invoke<PortalForm>('portal_get_form')
export const portalCreateKey = (name: string, unit?: string | null) =>
  invoke<CreatedKey>('portal_create_key', { name, unit: unit ?? null })
export const portalListKeys = () => invoke<PortalKeyChoice[]>('portal_list_keys')
export const portalUseKey = (id: number) => invoke<CreatedKey>('portal_use_key', { id })
export const portalClose = () => invoke<void>('portal_close')
export const restrictProviderConfig = () => invoke<void>('restrict_provider_config')
export const onPortalLogin = (handler: (result: PortalLogin) => void): Promise<UnlistenFn> =>
  listen<PortalLogin>('portal-login', (event) => handler(event.payload))
export const onBackendLog = (handler: (log: BackendLog) => void): Promise<UnlistenFn> =>
  listen<BackendLog>('backend-log', (event) => handler(event.payload))
