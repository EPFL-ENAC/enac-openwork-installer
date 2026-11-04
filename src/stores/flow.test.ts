import { beforeEach, describe, expect, it } from 'vitest'
import type { StepId } from '../lib/flow'
import type { StepRun, TraceSink, TraceStepStatus } from '../lib/orchestrator'
import type { KeyAnswer, KeyPrompt, RunnerBridge } from '../lib/runners'
import { createFlowStore } from './flow'

const sink = (): TraceSink & { statuses: Partial<Record<StepId, TraceStepStatus>>; lines: string[] } => {
  const statuses: Partial<Record<StepId, TraceStepStatus>> = {}
  const lines: string[] = []
  return {
    statuses,
    lines,
    log: (_scope, message) => lines.push(message),
    setStepStatus: (step, status) => {
      statuses[step] = status
    },
  }
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 5))

let pressed = 0

const steps = (bridge: RunnerBridge): StepRun[] => [
  { id: 'network', run: async () => ({ status: 'passed' }) },
  {
    id: 'rcpKey',
    run: async () => {
      pressed += 1
      const answer = await bridge.askKeys({
        units: [],
        availableKeys: [],
        showUnitPicker: false,
        showExistingKeys: false,
        unit: null,
        branch: 'existing',
        suggestedKeyName: 'OpenWork-2',
        manual: false,
      })
      return { status: 'passed', result: { token: answer.token, keyName: 'OpenWork-2' } }
    },
  },
  {
    id: 'openwork',
    run: async () => {
      const token = bridge.results().rcpKey?.token
      return token
        ? { status: 'passed', result: { path: '~/.config/opencode/opencode.json' } }
        : { status: 'failed', error: { kind: 'missing-key', message: 'no token' } }
    },
  },
]

beforeEach(() => {
  pressed = 0
})

describe('flow store', () => {
  it('starts idle and drives the orchestrator on start', async () => {
    const trace = sink()
    const store = createFlowStore(steps, trace)

    expect(store.state.value).toBe('idle')
    expect(store.failedStep.value).toBeNull()

    const running = store.start()
    await tick()

    expect(store.state.value).toBe('running')
    expect(store.keyPrompt.value?.branch).toBe('existing')
    expect(trace.statuses.welcome).toBe('passed')
    expect(trace.statuses.network).toBe('passed')
    expect(trace.statuses.rcpKey).toBe('running')

    store.answerKeys({ token: 'tok-123456789' } as KeyAnswer)
    await running

    expect(store.state.value).toBe('done')
    expect(store.keyPrompt.value).toBeNull()
    expect(store.results.rcpKey).toEqual({
      status: 'passed',
      token: 'tok-123456789',
      keyName: 'OpenWork-2',
    })
    expect(store.results.openwork).toEqual({
      status: 'passed',
      path: '~/.config/opencode/opencode.json',
    })
    expect(trace.statuses.summary).toBe('passed')
  })

  it('stops on a failed step and retries it', async () => {
    const trace = sink()
    let attempts = 0
    const retryable = (bridge: RunnerBridge): StepRun[] => {
      void bridge
      return [
        { id: 'network', run: async () => ({ status: 'passed' }) },
        {
          id: 'openwork',
          run: async () => {
            attempts += 1
            return attempts === 1
              ? { status: 'failed', error: { kind: 'install', message: 'boom' } }
              : { status: 'passed', result: { version: '1.2.3' } }
          },
        },
      ]
    }
    const store = createFlowStore(retryable, trace)

    await store.start()

    expect(store.state.value).toBe('failed')
    expect(store.failedStep.value).toBe('openwork')
    expect(store.results.openwork).toEqual({ status: 'failed', detail: 'boom', kind: 'install' })

    await store.retry()

    expect(store.state.value).toBe('done')
    expect(store.failedStep.value).toBeNull()
    expect(store.results.openwork).toEqual({ status: 'passed', version: '1.2.3' })
    expect(trace.statuses.openwork).toBe('passed')
  })

  it('ignores an answer when no pause is pending', () => {
    const store = createFlowStore(steps, sink())

    expect(() => store.answerKeys({ token: 'x' })).not.toThrow()
    expect(store.keyPrompt.value).toBeNull()
  })

  it('answers the running pause only once', async () => {
    const trace = sink()
    const store = createFlowStore(steps, trace)

    const running = store.start()
    await tick()
    const prompt = store.keyPrompt.value as KeyPrompt

    store.answerKeys({ token: 'tok-123456789' } as KeyAnswer)
    store.answerKeys({ token: 'tok-987654321' } as KeyAnswer)
    await running

    expect(prompt.suggestedKeyName).toBe('OpenWork-2')
    expect(pressed).toBe(1)
    expect(store.results.rcpKey?.token).toBe('tok-123456789')
  })
})
