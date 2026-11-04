import { describe, expect, it } from 'vitest'
import { STEP_ORDER } from './flow'

describe('flow step order', () => {
  it('runs the RCP key before the OpenWork step, with no standalone OpenCode step', () => {
    expect(STEP_ORDER).toEqual(['welcome', 'network', 'rcpKey', 'openwork', 'summary'])
  })
})
