import type { FlowResults, StepDonePayload, StepId } from './flow'
import type { AppError } from './ipc'

export type TraceStepStatus = 'pending' | 'running' | 'passed' | 'skipped' | 'failed'
export type TraceScope = StepId | 'flow'

export interface TraceSink {
  log(scope: TraceScope, message: string): void
  setStepStatus(step: StepId, status: TraceStepStatus): void
}

export type StepOutcome =
  | { status: 'passed' | 'skipped'; result?: StepDonePayload }
  | { status: 'failed'; error: AppError }

export interface StepRun {
  id: StepId
  run: (signals: { emit: (line: string) => void }) => Promise<StepOutcome>
}

export type OrchestratorState = 'idle' | 'running' | 'failed' | 'done'

export interface OrchestratorHooks {
  onComplete?: (results: FlowResults) => void
}

export interface Orchestrator {
  start: () => Promise<void>
  retry: () => Promise<void>
  readonly state: OrchestratorState
  readonly failedStep: StepId | null
  readonly results: FlowResults
}

export const createOrchestrator = (
  steps: StepRun[],
  sink: TraceSink,
  hooks: OrchestratorHooks = {},
): Orchestrator => {
  let state: OrchestratorState = 'idle'
  let failedStep: StepId | null = null
  let results: FlowResults = {}

  const attempt = async (step: StepRun): Promise<StepOutcome> => {
    try {
      return await step.run({ emit: (line) => sink.log(step.id, line) })
    } catch (error) {
      return {
        status: 'failed',
        error: { kind: 'unexpected', message: error instanceof Error ? error.message : String(error) },
      }
    }
  }

  const runFrom = async (from: number): Promise<void> => {
    state = 'running'
    for (let index = from; index < steps.length; index += 1) {
      const step = steps[index]
      sink.setStepStatus(step.id, 'running')
      const outcome = await attempt(step)
      if (outcome.status === 'failed') {
        results = {
          ...results,
          [step.id]: { status: 'failed', kind: outcome.error.kind, detail: outcome.error.message },
        }
        sink.setStepStatus(step.id, 'failed')
        failedStep = step.id
        state = 'failed'
        return
      }
      results = { ...results, [step.id]: { ...outcome.result, status: outcome.status } }
      sink.setStepStatus(step.id, outcome.status)
    }
    failedStep = null
    state = 'done'
    hooks.onComplete?.(results)
  }

  const start = async (): Promise<void> => {
    if (state === 'running') return
    steps.forEach((step) => sink.setStepStatus(step.id, 'pending'))
    results = {}
    failedStep = null
    await runFrom(0)
  }

  const retry = async (): Promise<void> => {
    if (state !== 'failed' || failedStep === null) return
    const from = steps.findIndex((step) => step.id === failedStep)
    if (from < 0) return
    const { [failedStep]: _dropped, ...kept } = results
    results = kept
    failedStep = null
    await runFrom(from)
  }

  return {
    start,
    retry,
    get state() {
      return state
    },
    get failedStep() {
      return failedStep
    },
    get results() {
      return results
    },
  }
}
