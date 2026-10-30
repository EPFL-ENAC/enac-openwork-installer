export type StepId =
  | 'welcome'
  | 'network'
  | 'opencode'
  | 'rcpKey'
  | 'providerConfig'
  | 'smokeTest'
  | 'openwork'
  | 'summary'

export type StepStatus = 'passed' | 'skipped' | 'failed'

export interface StepResult {
  status: StepStatus
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
  'opencode',
  'rcpKey',
  'providerConfig',
  'smokeTest',
  'openwork',
  'summary',
]

export const stepIndex = (id: StepId): number => STEP_ORDER.indexOf(id)

export const nextStep = (id: StepId): StepId | null => STEP_ORDER[stepIndex(id) + 1] ?? null

export const isStepUnlocked = (id: StepId, results: FlowResults): boolean => {
  const index = stepIndex(id)
  if (index <= 0) return true
  const previous = results[STEP_ORDER[index - 1]]
  return previous?.status === 'passed' || previous?.status === 'skipped'
}

export const unlockedSteps = (results: FlowResults): StepId[] =>
  STEP_ORDER.filter((id) => isStepUnlocked(id, results))

export const recordResult = (
  results: FlowResults,
  id: StepId,
  result: StepResult,
): FlowResults => ({ ...results, [id]: result })
