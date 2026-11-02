import { describe, expect, it } from 'vitest'
import type { AppError } from './ipc'
import type { StepDonePayload, StepId } from './flow'
import {
  createOrchestrator,
  type StepOutcome,
  type StepRun,
  type TraceScope,
  type TraceSink,
  type TraceStepStatus,
} from './orchestrator'

type Harness = {
  sink: TraceSink
  statuses: Partial<Record<StepId, TraceStepStatus>>
  logs: { scope: TraceScope; message: string }[]
}

const harness = (): Harness => {
  const statuses: Partial<Record<StepId, TraceStepStatus>> = {}
  const logs: { scope: TraceScope; message: string }[] = []
  return {
    statuses,
    logs,
    sink: {
      log: (scope, message) => logs.push({ scope, message }),
      setStepStatus: (step, status) => {
        statuses[step] = status
      },
    },
  }
}

const error: AppError = { kind: 'install', message: 'boom' }
const passed = (result?: StepDonePayload): StepOutcome => ({ status: 'passed', result })
const skipped = (result?: StepDonePayload): StepOutcome => ({ status: 'skipped', result })
const failed = (): StepOutcome => ({ status: 'failed', error })

describe('orchestrator order', () => {
  it('runs the steps in array order, each after the previous resolves', async () => {
    const { sink, statuses } = harness()
    const trace: string[] = []
    const step = (id: StepId): StepRun => ({
      id,
      run: async () => {
        trace.push(`${id}:start`)
        await new Promise((resolve) => setTimeout(resolve, 5))
        trace.push(`${id}:end`)
        return passed()
      },
    })

    const flow = createOrchestrator([step('welcome'), step('network'), step('opencode')], sink)
    await flow.start()

    expect(trace).toEqual([
      'welcome:start',
      'welcome:end',
      'network:start',
      'network:end',
      'opencode:start',
      'opencode:end',
    ])
    expect(statuses).toEqual({ welcome: 'passed', network: 'passed', opencode: 'passed' })
    expect(flow.state).toBe('done')
  })

  it('forwards emitted progress lines to the sink for the running step', async () => {
    const { sink, logs } = harness()
    const flow = createOrchestrator(
      [
        {
          id: 'welcome',
          run: async ({ emit }) => {
            emit('downloading')
            emit('verifying')
            return passed()
          },
        },
      ],
      sink,
    )

    await flow.start()

    expect(logs).toEqual([
      { scope: 'welcome', message: 'downloading' },
      { scope: 'welcome', message: 'verifying' },
    ])
  })
})

describe('orchestrator pause point', () => {
  it('holds the next step while a runner awaits user input', async () => {
    const { sink, statuses } = harness()
    const trace: string[] = []
    let release: () => void = () => {}
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })

    const flow = createOrchestrator(
      [
        { id: 'welcome', run: async () => passed() },
        {
          id: 'rcpKey',
          run: async () => {
            trace.push('rcpKey:waiting')
            await gate
            trace.push('rcpKey:resumed')
            return passed()
          },
        },
        { id: 'openwork', run: async () => (trace.push('openwork:run'), passed()) },
      ],
      sink,
    )

    const running = flow.start()
    await new Promise((resolve) => setTimeout(resolve, 5))

    expect(trace).toEqual(['rcpKey:waiting'])
    expect(statuses).toEqual({ welcome: 'passed', rcpKey: 'running', openwork: 'pending' })
    expect(flow.state).toBe('running')

    release()
    await running

    expect(trace).toEqual(['rcpKey:waiting', 'rcpKey:resumed', 'openwork:run'])
    expect(flow.state).toBe('done')
  })
})

describe('orchestrator failure', () => {
  it('stops on the first failure and leaves later steps pending', async () => {
    const { sink, statuses } = harness()
    const trace: string[] = []
    const flow = createOrchestrator(
      [
        { id: 'welcome', run: async () => passed() },
        { id: 'opencode', run: async () => failed() },
        {
          id: 'rcpKey',
          run: async () => {
            trace.push('rcpKey:run')
            return passed()
          },
        },
      ],
      sink,
    )

    await flow.start()

    expect(trace).toEqual([])
    expect(flow.state).toBe('failed')
    expect(flow.failedStep).toBe('opencode')
    expect(statuses).toEqual({ welcome: 'passed', opencode: 'failed', rcpKey: 'pending' })
  })
})

describe('orchestrator retry', () => {
  it('re-runs the failed step, keeps passed steps, and finishes the rest once', async () => {
    const { sink } = harness()
    const calls: Record<string, number> = {}
    const onComplete: StepId[] = []
    const bump = (id: StepId) => {
      calls[id] = (calls[id] ?? 0) + 1
      return calls[id]
    }
    const flow = createOrchestrator(
      [
        {
          id: 'welcome',
          run: async () => {
            bump('welcome')
            return passed({ detail: 'ok' })
          },
        },
        {
          id: 'opencode',
          run: async () => {
            const attempt = bump('opencode')
            return attempt === 1 ? failed() : passed({ version: '1.2.3' })
          },
        },
        {
          id: 'rcpKey',
          run: async () => {
            bump('rcpKey')
            return skipped()
          },
        },
        {
          id: 'openwork',
          run: async () => {
            bump('openwork')
            return passed({ ok: true })
          },
        },
      ],
      sink,
      { onComplete: (results) => onComplete.push(...(Object.keys(results) as StepId[])) },
    )

    await flow.start()
    expect(flow.state).toBe('failed')
    expect(flow.failedStep).toBe('opencode')

    await flow.retry()

    expect(calls).toEqual({ welcome: 1, opencode: 2, rcpKey: 1, openwork: 1 })
    expect(flow.state).toBe('done')
    expect(flow.failedStep).toBeNull()
    expect(flow.results).toEqual({
      welcome: { status: 'passed', detail: 'ok' },
      opencode: { status: 'passed', version: '1.2.3' },
      rcpKey: { status: 'skipped' },
      openwork: { status: 'passed', ok: true },
    })
    expect(onComplete).toEqual(['welcome', 'opencode', 'rcpKey', 'openwork'])
  })

  it('does nothing when retry runs in a non-failed state', async () => {
    const { sink } = harness()
    let runs = 0
    const flow = createOrchestrator(
      [
        {
          id: 'welcome',
          run: async () => {
            runs += 1
            return passed()
          },
        },
      ],
      sink,
    )

    await flow.retry()
    expect(flow.state).toBe('idle')
    expect(runs).toBe(0)

    await flow.start()
    await flow.retry()
    expect(flow.state).toBe('done')
    expect(runs).toBe(1)
  })
})

describe('orchestrator re-entry guard', () => {
  it('ignores start and retry while a run is in flight', async () => {
    const { sink, statuses } = harness()
    let runs = 0
    let release: () => void = () => {}
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })
    const flow = createOrchestrator(
      [
        {
          id: 'welcome',
          run: async () => {
            runs += 1
            await gate
            return passed()
          },
        },
        { id: 'network', run: async () => passed() },
      ],
      sink,
    )

    const running = flow.start()
    await new Promise((resolve) => setTimeout(resolve, 5))
    await flow.start()
    await flow.retry()
    expect(runs).toBe(1)
    expect(statuses).toEqual({ welcome: 'running', network: 'pending' })

    release()
    await running
    expect(flow.state).toBe('done')
  })
})
