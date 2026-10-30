import { reactive, ref, type Ref } from 'vue'
import type { FlowResults, StepId } from '../lib/flow'
import {
  createOrchestrator,
  type Orchestrator,
  type OrchestratorState,
  type StepRun,
  type TraceSink,
} from '../lib/orchestrator'
import { createRunners, type KeyAnswer, type KeyPrompt, type RunnerBridge } from '../lib/runners'
import { trace } from '../lib/trace'

export interface FlowStore {
  results: FlowResults
  state: Ref<OrchestratorState>
  failedStep: Ref<StepId | null>
  keyPrompt: Ref<KeyPrompt | null>
  start: () => Promise<void>
  retry: () => Promise<void>
  answerKeys: (answer: KeyAnswer) => void
}

/**
 * Builds the flow store. `steps` receives the runner bridge so the rcpKey pause
 * and the provider token share one source of truth.
 */
export const createFlowStore = (
  steps: (bridge: RunnerBridge) => StepRun[],
  sink: TraceSink = trace,
): FlowStore => {
  const results = reactive<FlowResults>({})
  const state = ref<OrchestratorState>('idle')
  const failedStep = ref<StepId | null>(null)
  const keyPrompt = ref<KeyPrompt | null>(null)
  let answerKey: ((answer: KeyAnswer) => void) | null = null

  const askKeys = (prompt: KeyPrompt): Promise<KeyAnswer> => {
    keyPrompt.value = prompt
    return new Promise<KeyAnswer>((resolve) => {
      answerKey = resolve
    })
  }

  let orchestrator: Orchestrator

  const sync = () => {
    const next = orchestrator.results
    for (const id of Object.keys(results) as StepId[]) {
      if (!(id in next)) delete results[id]
    }
    Object.assign(results, next)
    state.value = orchestrator.state
    failedStep.value = orchestrator.failedStep
  }

  const stepSink: TraceSink = {
    log: (scope, message) => sink.log(scope, message),
    setStepStatus: (step, status) => {
      sink.setStepStatus(step, status)
      sync()
    },
  }

  orchestrator = createOrchestrator(steps({ results: () => orchestrator.results, askKeys }), stepSink, {
    onComplete: () => {
      sink.setStepStatus('summary', 'passed')
      sync()
    },
  })

  const start = async () => {
    if (state.value === 'running') return
    sink.setStepStatus('welcome', 'passed')
    await orchestrator.start()
    sync()
  }

  const retry = async () => {
    if (state.value !== 'failed') return
    await orchestrator.retry()
    sync()
  }

  const answerKeys = (answer: KeyAnswer) => {
    const resolve = answerKey
    answerKey = null
    keyPrompt.value = null
    resolve?.(answer)
  }

  return { results, state, failedStep, keyPrompt, start, retry, answerKeys }
}

export const flow = createFlowStore((bridge) => createRunners(bridge))

export const useFlow = (): FlowStore => flow
