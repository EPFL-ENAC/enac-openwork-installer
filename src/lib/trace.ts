import { reactive } from 'vue'
import { STEP_ORDER, type StepId } from './flow'

export type TraceStepStatus = 'pending' | 'running' | 'passed' | 'skipped' | 'failed'

export type TraceScope = StepId | 'flow'

export interface TraceEntry {
  ts: number
  scope: TraceScope
  message: string
}

export interface TraceSink {
  log(scope: TraceScope, message: string): void
  setStepStatus(step: StepId, status: TraceStepStatus): void
}

export const MASK = '***'

const secrets = new Set<string>()

// Any length: a short token still identifies the user, so it is always masked.
export const registerSecret = (value: string): void => {
  if (value.length === 0) return
  secrets.add(value)
}

export const maskSecrets = (message: string): string => {
  const ordered = [...secrets].sort((a, b) => b.length - a.length)
  return ordered.reduce((text, secret) => text.split(secret).join(MASK), message)
}

const pad = (value: number): string => String(value).padStart(2, '0')

export const formatClock = (ts: number): string => {
  const date = new Date(ts)
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

const pendingStatuses = (): Record<StepId, TraceStepStatus> =>
  Object.fromEntries(STEP_ORDER.map((id) => [id, 'pending'])) as Record<StepId, TraceStepStatus>

const entries = reactive<TraceEntry[]>([])
const stepStatuses = reactive(pendingStatuses())

export const trace: TraceSink & {
  entries: TraceEntry[]
  stepStatuses: Record<StepId, TraceStepStatus>
  reset(): void
} = {
  entries,
  stepStatuses,
  log(scope, message) {
    entries.push({ ts: Date.now(), scope, message: maskSecrets(message) })
  },
  setStepStatus(step, status) {
    stepStatuses[step] = status
  },
  reset() {
    entries.splice(0, entries.length)
    Object.assign(stepStatuses, pendingStatuses())
  },
}
