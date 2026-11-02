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
  // An existing key keeps its unit, so only a new key needs the unit choice.
  const showUnitPicker = input.units.length > 1 && !input.existingOpenWorkKey
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
    // An existing OpenWork key is reused: its stored secret is revealed, so the default
    // name stays. Only a refused name asks for a second key.
    suggestedKeyName: branch === 'refused' ? SECOND_KEY_NAME : DEFAULT_KEY_NAME,
  }
}
