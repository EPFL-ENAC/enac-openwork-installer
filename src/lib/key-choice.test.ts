import { describe, expect, it } from 'vitest'
import { chooseKeyBranch, poolLabel } from './key-choice'

describe('chooseKeyBranch', () => {
  it('creates a fresh key when no OpenWork key exists', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC'],
      existingOpenWorkKey: false,
      refused: false,
    })

    expect(choice.branch).toBe('fresh')
    expect(choice.suggestedKeyName).toBe('OpenWork')
  })

  it('auto-selects the only unit and hides the picker', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC'],
      existingOpenWorkKey: false,
      refused: false,
    })

    expect(choice.unit).toBe('ENAC')
    expect(choice.showUnitPicker).toBe(false)
  })

  it('preselects the only unit when the list repeats it', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC', 'ENAC'],
      existingOpenWorkKey: false,
      refused: false,
    })

    expect(choice.unit).toBe('ENAC')
    expect(choice.showUnitPicker).toBe(false)
  })

  it('ignores blank unit entries', () => {
    const choice = chooseKeyBranch({
      units: ['', 'ENAC', '  '],
      existingOpenWorkKey: false,
      refused: false,
    })

    expect(choice.unit).toBe('ENAC')
    expect(choice.showUnitPicker).toBe(false)
  })

  it('shows the picker when a repeated unit joins a distinct one', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC', 'ENAC', 'STI'],
      existingOpenWorkKey: false,
      refused: false,
    })

    expect(choice.showUnitPicker).toBe(true)
    expect(choice.unit).toBeNull()
  })

  it('shows the picker and keeps no unit when several units exist', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC', 'STI'],
      existingOpenWorkKey: false,
      refused: false,
    })

    expect(choice.showUnitPicker).toBe(true)
    expect(choice.unit).toBeNull()
  })

  it('keeps a chosen unit that belongs to the unit list', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC', 'STI'],
      existingOpenWorkKey: false,
      refused: false,
      chosenUnit: 'STI',
    })

    expect(choice.unit).toBe('STI')
  })

  it('drops a chosen unit that is not in the unit list', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC', 'STI'],
      existingOpenWorkKey: false,
      refused: false,
      chosenUnit: 'GONE',
    })

    expect(choice.unit).toBeNull()
  })

  it('has no unit when the portal lists none', () => {
    const choice = chooseKeyBranch({ units: [], existingOpenWorkKey: false, refused: false })

    expect(choice.unit).toBeNull()
    expect(choice.showUnitPicker).toBe(false)
  })

  it('reuses the reported existing key under its own name', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC'],
      existingOpenWorkKey: true,
      refused: false,
    })

    expect(choice.branch).toBe('existing')
    expect(choice.suggestedKeyName).toBe('OpenWork')
  })

  it('asks no unit for an existing key even when several units exist', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC', 'STI'],
      existingOpenWorkKey: true,
      refused: false,
    })

    expect(choice.showUnitPicker).toBe(false)
    expect(choice.unit).toBe('ENAC')
  })

  it('offers the existing-key choice when the portal refuses a duplicate name', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC'],
      existingOpenWorkKey: false,
      refused: true,
    })

    expect(choice.branch).toBe('refused')
    expect(choice.suggestedKeyName).toBe('OpenWork-2')
  })

  it('prefers the refusal branch over the reported existing key', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC'],
      existingOpenWorkKey: true,
      refused: true,
    })

    expect(choice.branch).toBe('refused')
  })

  it('does not mutate its inputs', () => {
    const units = ['ENAC', 'STI']
    chooseKeyBranch({ units, existingOpenWorkKey: false, refused: false, chosenUnit: 'ENAC' })

    expect(units).toEqual(['ENAC', 'STI'])
  })

  it('offers the selectable keys on the fresh branch', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC'],
      existingOpenWorkKey: false,
      refused: false,
      selectableKeyCount: 2,
    })

    expect(choice.branch).toBe('fresh')
    expect(choice.showExistingKeys).toBe(true)
  })

  it('hides the key list when no key is selectable', () => {
    expect(
      chooseKeyBranch({ units: ['ENAC'], existingOpenWorkKey: false, refused: false, selectableKeyCount: 0 })
        .showExistingKeys,
    ).toBe(false)
    expect(chooseKeyBranch({ units: ['ENAC'], existingOpenWorkKey: false, refused: false }).showExistingKeys).toBe(
      false,
    )
  })

  it('hides the key list on the existing and refused branches', () => {
    const existing = chooseKeyBranch({
      units: ['ENAC'],
      existingOpenWorkKey: true,
      refused: false,
      selectableKeyCount: 2,
    })
    const refused = chooseKeyBranch({
      units: ['ENAC'],
      existingOpenWorkKey: false,
      refused: true,
      selectableKeyCount: 2,
    })

    expect(existing.showExistingKeys).toBe(false)
    expect(refused.showExistingKeys).toBe(false)
  })

  it('creates a fresh key with the explicit new name over an existing OpenWork key', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC', 'STI'],
      existingOpenWorkKey: true,
      refused: false,
      requestedKeyName: 'MyKey',
    })

    expect(choice.branch).toBe('fresh')
    expect(choice.suggestedKeyName).toBe('MyKey')
    expect(choice.showUnitPicker).toBe(true)
    expect(choice.unit).toBeNull()
    expect(choice.showExistingKeys).toBe(false)
  })

  it('keeps the explicit create request after a refusal instead of the paste branch', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC'],
      existingOpenWorkKey: true,
      refused: true,
      requestedKeyName: 'MyKey',
    })

    expect(choice.branch).toBe('fresh')
    expect(choice.suggestedKeyName).toBe('MyKey')
  })

  it('treats the default name as no explicit request', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC'],
      existingOpenWorkKey: true,
      refused: false,
      requestedKeyName: 'OpenWork',
    })

    expect(choice.branch).toBe('existing')
    expect(choice.suggestedKeyName).toBe('OpenWork')
  })
})

describe('poolLabel', () => {
  it('labels the premium pool and defaults to basic', () => {
    expect(poolLabel('PREMIUM')).toBe('premium')
    expect(poolLabel('premium')).toBe('premium')
    expect(poolLabel('BASIC')).toBe('basic')
    expect(poolLabel('')).toBe('basic')
  })
})
