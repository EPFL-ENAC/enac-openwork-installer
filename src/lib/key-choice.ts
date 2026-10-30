export const DEFAULT_KEY_NAME = 'OpenWork'
export const SECOND_KEY_NAME = 'OpenWork-2'

export type KeyBranch = 'fresh' | 'existing' | 'refused'

export interface KeyChoiceInput {
  units: readonly string[]
  existingOpenWorkKey: boolean
  refused: boolean
  chosenUnit?: string | null
}

export interface KeyChoice {
  branch: KeyBranch
  unit: string | null
  showUnitPicker: boolean
  suggestedKeyName: string
}

export const chooseKeyBranch = (input: KeyChoiceInput): KeyChoice => {
  const showUnitPicker = input.units.length > 1
  const chosen = input.chosenUnit
  const unit = showUnitPicker
    ? chosen && input.units.includes(chosen)
      ? chosen
      : null
    : (input.units[0] ?? null)
  const branch: KeyBranch = input.refused
    ? 'refused'
    : input.existingOpenWorkKey
      ? 'existing'
      : 'fresh'

  return {
    branch,
    unit,
    showUnitPicker,
    suggestedKeyName: branch === 'fresh' ? DEFAULT_KEY_NAME : SECOND_KEY_NAME,
  }
}
