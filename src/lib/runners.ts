import type { FlowResults, StepDonePayload, StepId } from './flow'
import type { StepOutcome, StepRun } from './orchestrator'
import { NETWORK_TIMEOUT_MS, PORTAL_URL } from './links'
import { DEFAULT_KEY_NAME, chooseKeyBranch, type KeyChoice } from './key-choice'
import {
  installOpencode,
  installOpenwork,
  onBackendLog,
  onPortalLogin,
  opencodeStatus,
  openworkStateStatus,
  openworkStatus,
  portalClose,
  portalCreateKey,
  portalGetForm,
  portalStartLogin,
  smokeTest,
  type AppError,
  type BackendLog,
  type InstallResult,
  type InstallStatus,
  type PortalForm,
  type PortalLogin,
} from './ipc'
import { writeProviderConfig } from './provider-config'
import { registerSecret } from './trace'

/** Input the rcpKey card must collect before the runner can continue. */
export interface KeyPrompt extends KeyChoice {
  units: string[]
  /** The portal window failed, so only a pasted token can help. */
  manual: boolean
}

export interface KeyAnswer {
  token?: string
  keyName?: string
  unit?: string | null
}

export interface RunnerBridge {
  results: () => FlowResults
  askKeys: (prompt: KeyPrompt) => Promise<KeyAnswer>
}

const toAppError = (cause: unknown): AppError => {
  const failure = cause as Partial<AppError> | null
  return {
    kind: typeof failure?.kind === 'string' ? failure.kind : 'unexpected',
    message: typeof failure?.message === 'string' ? failure.message : String(cause),
  }
}

const failed = (error: AppError): StepOutcome => ({ status: 'failed', error })

const passed = (result?: StepDonePayload): StepOutcome => ({ status: 'passed', result })

const deferred = <T>(): { promise: Promise<T>; resolve: (value: T) => void } => {
  let resolve: (value: T) => void = () => {}
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

const networkRun = async (emit: (line: string) => void): Promise<StepOutcome> => {
  emit(`Checking ${PORTAL_URL}`)
  try {
    // no-cors: the opaque response is enough to prove the portal is reachable.
    await fetch(PORTAL_URL, { mode: 'no-cors', signal: AbortSignal.timeout(NETWORK_TIMEOUT_MS) })
    emit('The EPFL portal is reachable')
    return passed()
  } catch {
    emit('The EPFL portal is not reachable')
    return failed({ kind: 'network', message: 'The EPFL portal is not reachable.' })
  }
}

const toolRun = async (
  id: 'opencode' | 'openwork',
  label: string,
  status: () => Promise<InstallStatus>,
  install: () => Promise<InstallResult>,
  emit: (line: string) => void,
): Promise<StepOutcome> => {
  let current: InstallStatus
  try {
    current = await status()
  } catch (cause) {
    emit(`${label} status check failed`)
    return failed(toAppError(cause))
  }

  if (current.installed) {
    emit(`${label} ${current.version ?? ''} is already installed, skip`)
    return {
      status: 'skipped',
      result: { version: current.version ?? undefined, path: current.path ?? undefined },
    }
  }

  emit(`${label} is not installed, the installation starts`)
  let unlisten: (() => void) | null = null
  try {
    unlisten = await onBackendLog((log: BackendLog) => {
      if (log.task === id) emit(log.message)
    })
  } catch {
    // No event bridge outside the Tauri shell. The installation still runs.
  }

  try {
    const result = await install()
    emit(`${label} ${result.version} is installed`)
    return passed({ version: result.version, path: result.path })
  } catch (cause) {
    emit(`${label} installation failed`)
    return failed(toAppError(cause))
  } finally {
    unlisten?.()
  }
}

const acceptToken = (
  token: string,
  keyName: string | undefined,
  emit: (line: string) => void,
): StepOutcome => {
  const trimmed = token.trim()
  registerSecret(trimmed)
  emit(`Using the RCP key ${keyName ?? DEFAULT_KEY_NAME}`)
  return passed({ keyName: keyName?.trim() || DEFAULT_KEY_NAME, token: trimmed })
}

const manualFallback = async (
  bridge: RunnerBridge,
  emit: (line: string) => void,
  reason: string,
): Promise<StepOutcome> => {
  emit(`The portal login did not complete: ${reason}`)
  emit('Waiting for a pasted RCP token')
  const answer = await bridge.askKeys({
    units: [],
    showUnitPicker: false,
    unit: null,
    branch: 'existing',
    suggestedKeyName: DEFAULT_KEY_NAME,
    manual: true,
  })
  if (!answer.token?.trim()) return failed({ kind: 'portal-auth', message: reason })
  return acceptToken(answer.token, answer.keyName, emit)
}

const rcpKeyRun = async (
  bridge: RunnerBridge,
  emit: (line: string) => void,
): Promise<StepOutcome> => {
  const login = deferred<PortalLogin>()
  let unlisten: (() => void) | null = null
  try {
    unlisten = await onPortalLogin((result) => login.resolve(result))
  } catch {
    // No event bridge outside the Tauri shell. The manual fallback stays usable.
  }

  try {
    emit('Opening the RCP portal window')
    try {
      await portalStartLogin()
      emit('Waiting for the EPFL login')
    } catch (cause) {
      return await manualFallback(bridge, emit, toAppError(cause).message)
    }

    const result = await login.promise
    if (!result.ok) return await manualFallback(bridge, emit, result.error ?? 'login failed')

    emit('Reading the RCP portal form')
    let form: PortalForm
    try {
      form = await portalGetForm()
    } catch (cause) {
      return await manualFallback(bridge, emit, toAppError(cause).message)
    }

    const units = form.units ?? []
    const ask = async (choice: KeyChoice): Promise<KeyAnswer> =>
      bridge.askKeys({ ...choice, units, manual: false })

    let choice = chooseKeyBranch({
      units,
      existingOpenWorkKey: form.existingOpenWorkKey,
      refused: false,
    })
    emit(`The portal offers ${units.length} unit(s), existing OpenWork key: ${form.existingOpenWorkKey}`)

    let answer: KeyAnswer = {}
    let unit = choice.unit
    if (choice.showUnitPicker) answer = await ask(choice)

    for (;;) {
      if (answer.token?.trim()) return acceptToken(answer.token, answer.keyName ?? choice.suggestedKeyName, emit)

      unit = answer.unit ?? unit
      const keyName = answer.keyName?.trim() || choice.suggestedKeyName
      emit(
        choice.branch === 'existing'
          ? `Reusing the existing RCP key ${keyName}`
          : `Creating the RCP key ${keyName}`,
      )
      try {
        const created = await portalCreateKey(keyName, unit)
        registerSecret(created.token)
        emit(`The RCP key ${created.name} is ready`)
        return passed({ keyName: created.name, token: created.token })
      } catch (cause) {
        const error = toAppError(cause)
        if (error.kind !== 'portal-refused') {
          emit(`The RCP key could not be created: ${error.message}`)
          return failed(error)
        }
        choice = chooseKeyBranch({
          units,
          existingOpenWorkKey: form.existingOpenWorkKey,
          refused: true,
          chosenUnit: unit,
        })
        emit(`The portal refused the key name ${keyName}`)
        answer = await ask(choice)
      }
    }
  } finally {
    unlisten?.()
    portalClose().catch(() => {})
  }
}

const providerRun = async (
  bridge: RunnerBridge,
  emit: (line: string) => void,
): Promise<StepOutcome> => {
  const token = bridge.results().rcpKey?.token
  if (!token) {
    emit('No RCP key is available')
    return failed({
      kind: 'missing-key',
      message: 'No RCP key is available from the previous step.',
    })
  }

  emit('Writing the RCP provider into the OpenCode configuration')
  const outcome = await writeProviderConfig(token)
  if (!outcome.ok) {
    emit(`The OpenCode configuration was not written: ${outcome.detail}`)
    return failed({ kind: outcome.code, message: outcome.detail })
  }

  emit(outcome.changed ? `The configuration is written to ${outcome.configPath}` : 'The RCP provider is already configured')
  return passed({ path: outcome.configPath })
}

const smokeRun = async (
  bridge: RunnerBridge,
  emit: (line: string) => void,
): Promise<StepOutcome> => {
  emit('Running the smoke test')
  try {
    const result = await smokeTest(bridge.results().rcpKey?.token ?? null)
    if (result.ok) {
      emit('The smoke test passed')
      return passed({ ok: true, detail: result.detail ?? undefined })
    }
    const detail = result.error ?? result.detail ?? 'unknown error'
    emit(`The smoke test failed: ${detail}`)
    return failed({ kind: 'smoke', message: detail })
  } catch (cause) {
    emit('The smoke test could not run')
    return failed(toAppError(cause))
  }
}

const opencodeRun = async (
  bridge: RunnerBridge,
  emit: (line: string) => void,
): Promise<StepOutcome> => {
  const install = await toolRun('opencode', 'OpenCode', opencodeStatus, installOpencode, emit)
  if (install.status === 'failed') return install

  const config = await providerRun(bridge, emit)
  if (config.status === 'failed') return config

  const smoke = await smokeRun(bridge, emit)
  if (smoke.status === 'failed') return smoke

  return passed({ ...install.result, ...config.result, ...smoke.result })
}

const openworkRun = async (
  bridge: RunnerBridge,
  emit: (line: string) => void,
): Promise<StepOutcome> => {
  try {
    const state = await openworkStateStatus()
    if (state.broken) {
      emit(`The OpenWork database blocks the start: ${state.databases.join(', ')}`)
      return failed({ kind: 'openwork-broken', message: state.databases.join(', ') })
    }
  } catch {
    // A state probe that does not answer must not block the installation.
  }
  return toolRun('openwork', 'OpenWork', openworkStatus, installOpenwork, emit)
}

export const createRunners = (bridge: RunnerBridge): StepRun[] => {
  const step = (
    id: StepId,
    run: (emit: (line: string) => void) => Promise<StepOutcome>,
  ): StepRun => ({ id, run: ({ emit }) => run(emit) })

  return [
    step('network', networkRun),
    step('rcpKey', (emit) => rcpKeyRun(bridge, emit)),
    step('opencode', (emit) => opencodeRun(bridge, emit)),
    step('openwork', (emit) => openworkRun(bridge, emit)),
  ]
}
