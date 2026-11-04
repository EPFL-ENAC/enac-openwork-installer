import type { FlowResults, StepDonePayload, StepId } from './flow'
import type { StepOutcome, StepRun } from './orchestrator'
import { NETWORK_TIMEOUT_MS, PORTAL_URL } from './links'
import { DEFAULT_KEY_NAME, chooseKeyBranch, type KeyChoice } from './key-choice'
import {
  installOpenwork,
  configureOpenwork,
  onBackendLog,
  onPortalLogin,
  openworkStateStatus,
  openworkStatus,
  portalClose,
  portalCreateKey,
  portalGetForm,
  portalListKeys,
  portalStartLogin,
  portalUseKey,
  smokeTest,
  type AppError,
  type BackendLog,
  type InstallResult,
  type InstallStatus,
  type PortalForm,
  type PortalKeyChoice,
  type PortalLogin,
} from './ipc'
import { writeProviderConfig } from './provider-config'
import { registerSecret } from './trace'

/** Input the rcpKey card must collect before the runner can continue. */
export interface KeyPrompt extends KeyChoice {
  units: string[]
  /** The user's own keys the portal offers for reuse. */
  availableKeys: PortalKeyChoice[]
  /** The portal window failed, so only a pasted token can help. */
  manual: boolean
  /** Why the last create attempt failed, shown above the same create form. */
  error?: string | null
}

export interface KeyAnswer {
  token?: string
  keyName?: string
  unit?: string | null
  /** The id of an existing portal key the user chose to reuse. */
  keyId?: number
}

export interface RunnerBridge {
  results: () => FlowResults
  askKeys: (prompt: KeyPrompt) => Promise<KeyAnswer>
}

/** The provider id inside OpenWork (the OpenCode config uses epfl-rcp-aiaas). */
const OPENWORK_PROVIDER_ID = 'epfl-aiaas'

const toAppError = (cause: unknown): AppError => {
  const failure = cause as Partial<AppError> | null
  return {
    kind: typeof failure?.kind === 'string' ? failure.kind : 'unexpected',
    message: typeof failure?.message === 'string' ? failure.message : String(cause),
  }
}

const failed = (error: AppError): StepOutcome => ({ status: 'failed', error })

/** Names the cause of a create failure and hints at the next step. */
const createFailureMessage = (error: AppError): string => {
  const message = error.message.trim()
  if (error.kind === 'portal-auth') return `${message} Sign in to the portal again, then retry.`
  if (error.kind === 'network') return `${message} Check the network connection and retry.`
  return message
}

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
  id: 'openwork',
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
    availableKeys: [],
    showUnitPicker: false,
    showExistingKeys: false,
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
    // A list that does not load must not block the create path.
    let availableKeys: PortalKeyChoice[] = []
    try {
      availableKeys = await portalListKeys()
    } catch {
      emit('The portal key list did not load')
    }
    const ask = async (choice: KeyChoice, error?: string | null): Promise<KeyAnswer> =>
      bridge.askKeys({ ...choice, units, availableKeys, manual: false, error })

    let choice = chooseKeyBranch({
      units,
      existingOpenWorkKey: form.existingOpenWorkKey,
      refused: false,
      selectableKeyCount: availableKeys.length,
    })
    emit(`The portal offers ${units.length} unit(s), existing OpenWork key: ${form.existingOpenWorkKey}`)

    let answer: KeyAnswer = {}
    let unit = choice.unit
    if (choice.showUnitPicker || choice.showExistingKeys) answer = await ask(choice)

    for (;;) {
      if (answer.token?.trim()) return acceptToken(answer.token, answer.keyName ?? choice.suggestedKeyName, emit)

      unit = answer.unit ?? unit
      const keyName = answer.keyName?.trim() || choice.suggestedKeyName
      const chosenId = answer.keyId
      emit(
        chosenId === undefined
          ? `Creating the RCP key ${keyName}`
          : `Reusing the existing RCP key ${keyName}`,
      )
      try {
        const created =
          chosenId === undefined ? await portalCreateKey(keyName, unit) : await portalUseKey(chosenId)
        registerSecret(created.token)
        emit(`The RCP key ${created.name} is ready`)
        return passed({ keyName: created.name, token: created.token })
      } catch (cause) {
        const error = toAppError(cause)
        const message = createFailureMessage(error)
        emit(`The RCP key could not be created: ${error.message}`)
        if (error.kind === 'portal-refused') {
          choice = chooseKeyBranch({
            units,
            existingOpenWorkKey: form.existingOpenWorkKey,
            refused: true,
            chosenUnit: unit,
            selectableKeyCount: availableKeys.length,
            // A name the user already gave stays a create request on the next prompt.
            requestedKeyName: answer.keyName,
          })
          emit(`The portal refused the key name ${keyName}`)
        } else {
          // Any other failure keeps the same create form, with the name, the unit and
          // the key list still offered. A manual paste never replaces it.
          choice = {
            ...choice,
            branch: 'fresh',
            unit,
            showExistingKeys: availableKeys.length > 0,
            suggestedKeyName: keyName,
          }
          emit('The key choice stays open, retry with the same name')
        }
        answer = await ask(choice, message)
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
  return passed({ configPath: outcome.configPath })
}

const smokeRun = async (
  bridge: RunnerBridge,
  freshInstall: boolean,
  emit: (line: string) => void,
): Promise<StepOutcome> => {
  emit('Running the smoke test on the OpenWork engine')
  try {
    const result = await smokeTest(bridge.results().rcpKey?.token ?? null, freshInstall)
    if (result.skipped) {
      const detail = result.detail ?? 'the OpenWork engine was not found'
      emit(`The engine check is skipped: ${detail}`)
      return { status: 'warning', result: { ok: false, detail } }
    }
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

const configureRun = async (
  token: string,
  emit: (line: string) => void,
): Promise<StepOutcome> => {
  emit('Configuring OpenWork to use the RCP provider')
  try {
    const outcome = await configureOpenwork(token)
    emit(
      outcome.providerWritten
        ? `The ${OPENWORK_PROVIDER_ID} provider is written into the OpenWork configuration`
        : `The ${OPENWORK_PROVIDER_ID} provider is already configured in OpenWork`,
    )
    emit(
      outcome.envWritten
        ? 'The RCP key is stored in the OpenWork environment store'
        : 'The RCP key is already stored in the OpenWork environment store',
    )
    emit(
      outcome.analyticsWritten
        ? 'The OpenWork anonymous usage data is turned off'
        : 'The OpenWork anonymous usage data is already off',
    )
    emit(
      outcome.workspaceConfigWritten
        ? `The stale ${OPENWORK_PROVIDER_ID} provider is removed from the workspace opencode.jsonc`
        : `The workspace opencode.jsonc carries no ${OPENWORK_PROVIDER_ID} provider override`,
    )
    emit(
      outcome.defaultModelWritten
        ? `The default model is set to ${OPENWORK_PROVIDER_ID}/large`
        : `The default model ${OPENWORK_PROVIDER_ID}/large is already set`,
    )
    return passed({ configDir: outcome.configDir })
  } catch (cause) {
    emit('The OpenWork configuration failed')
    return failed(toAppError(cause))
  }
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

  const install = await toolRun('openwork', 'OpenWork', openworkStatus, installOpenwork, emit)
  if (install.status === 'failed') return install

  const token = bridge.results().rcpKey?.token
  if (!token) {
    emit('No RCP key is available')
    return failed({
      kind: 'missing-key',
      message: 'No RCP key is available from the previous step.',
    })
  }

  const config = await configureRun(token, emit)
  if (config.status === 'failed') return config

  const provider = await providerRun(bridge, emit)
  if (provider.status === 'failed') return provider

  // An OpenWork installed by this run ships the v2 engine, an older install keeps v1.
  const smoke = await smokeRun(bridge, install.status === 'passed', emit)
  if (smoke.status === 'failed') return smoke

  return {
    status: smoke.status === 'warning' ? 'warning' : 'passed',
    result: { ...install.result, ...config.result, ...provider.result, ...smoke.result },
  }
}

export const createRunners = (bridge: RunnerBridge): StepRun[] => {
  const step = (
    id: StepId,
    run: (emit: (line: string) => void) => Promise<StepOutcome>,
  ): StepRun => ({ id, run: ({ emit }) => run(emit) })

  return [
    step('network', networkRun),
    step('rcpKey', (emit) => rcpKeyRun(bridge, emit)),
    step('openwork', (emit) => openworkRun(bridge, emit)),
  ]
}
