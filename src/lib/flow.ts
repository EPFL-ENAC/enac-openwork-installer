export type StepId =
  | 'welcome'
  | 'network'
  | 'rcpKey'
  | 'opencode'
  | 'openwork'
  | 'summary'

export type StepStatus = 'passed' | 'skipped' | 'failed'

export interface StepResult {
  status: StepStatus
  kind?: string
  version?: string
  path?: string
  ok?: boolean
  detail?: string
  keyName?: string
  token?: string
}

export type StepDonePayload = Partial<Omit<StepResult, 'status'>> & { status?: StepStatus }

export type FlowResults = Partial<Record<StepId, StepResult>>

export const STEP_ORDER: readonly StepId[] = [
  'welcome',
  'network',
  'rcpKey',
  'opencode',
  'openwork',
  'summary',
]
