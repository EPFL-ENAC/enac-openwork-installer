export const DEFAULT_KEY_NAME = 'OpenWork'
export const SECOND_KEY_NAME = 'OpenWork-2'

export type KeyBranch = 'fresh' | 'existing' | 'refused'
export type KeyPoolLabel = 'basic' | 'premium'

export interface KeyChoiceInput {
  units: readonly string[]
  existingOpenWorkKey: boolean
  refused: boolean
  chosenUnit?: string | null
  /** How many of the user's own keys the portal offers for reuse. */
  selectableKeyCount?: number
}

export interface KeyChoice {
  branch: KeyBranch
  unit: string | null
  showUnitPicker: boolean
  showExistingKeys: boolean
  suggestedKeyName: string
}

/** The pool of a portal key, as the label the card shows. */
export const poolLabel = (pool: string): KeyPoolLabel =>
  pool.trim().toLowerCase() === 'premium' ? 'premium' : 'basic'

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
    // Only a fresh branch offers a reuse list: an existing OpenWork key and a refused
    // name already have their own path.
    showExistingKeys: branch === 'fresh' && (input.selectableKeyCount ?? 0) > 0,
    // An existing OpenWork key is reused: its stored secret is revealed, so the default
    // name stays. Only a refused name asks for a second key.
    suggestedKeyName: branch === 'refused' ? SECOND_KEY_NAME : DEFAULT_KEY_NAME,
  }
}
