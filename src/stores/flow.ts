import { reactive } from 'vue'
import {
  isStepUnlocked,
  recordResult,
  type FlowResults,
  type StepDonePayload,
  type StepId,
  type StepResult,
} from '../lib/flow'

const results = reactive<FlowResults>({})

export const useFlow = () => {
  const isUnlocked = (id: StepId) => isStepUnlocked(id, results)

  const completeStep = (id: StepId, payload: StepDonePayload = {}) => {
    const result: StepResult = { status: 'passed', ...payload }
    Object.assign(results, recordResult(results, id, result))
  }

  return { results, isUnlocked, completeStep }
}
