import { describe, expect, it } from 'vitest'
import { chooseKeyBranch } from './key-choice'

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

  it('offers the existing-key choice when the portal reports a key', () => {
    const choice = chooseKeyBranch({
      units: ['ENAC'],
      existingOpenWorkKey: true,
      refused: false,
    })

    expect(choice.branch).toBe('existing')
    expect(choice.suggestedKeyName).toBe('OpenWork-2')
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
})
