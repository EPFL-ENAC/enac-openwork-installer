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
  /** The key name the user has committed to create. */
  requestedKeyName?: string | null
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

/** The units the picker can render: one per distinct, non-blank label. */
export const renderedUnits = (units: readonly string[]): string[] => [
  ...new Set(units.map((unit) => unit.trim()).filter((unit) => unit.length > 0)),
]

export const chooseKeyBranch = (input: KeyChoiceInput): KeyChoice => {
  const units = renderedUnits(input.units)
  // A name other than the default is an explicit create request: an existing OpenWork key
  // and a refusal of another name are then irrelevant.
  const requested = input.requestedKeyName?.trim()
  const explicitName = requested && requested !== DEFAULT_KEY_NAME ? requested : null
  // An existing key keeps its unit, so only a new key needs the unit choice.
  const showUnitPicker =
    units.length > 1 && (explicitName !== null || !input.existingOpenWorkKey)
  const chosen = input.chosenUnit
  const unit = showUnitPicker
    ? chosen && units.includes(chosen)
      ? chosen
      : null
    : (units[0] ?? null)
  const branch: KeyBranch = explicitName
    ? 'fresh'
    : input.refused
      ? 'refused'
      : input.existingOpenWorkKey
        ? 'existing'
        : 'fresh'

  return {
    branch,
    unit,
    showUnitPicker,
    // Only a fresh branch offers a reuse list: an explicit name, an existing OpenWork key
    // and a refused name already have their own path.
    showExistingKeys:
      branch === 'fresh' && explicitName === null && (input.selectableKeyCount ?? 0) > 0,
    // An existing OpenWork key is reused: its stored secret is revealed, so the default
    // name stays. Only a refused name asks for a second key.
    suggestedKeyName: explicitName ?? (branch === 'refused' ? SECOND_KEY_NAME : DEFAULT_KEY_NAME),
  }
}
