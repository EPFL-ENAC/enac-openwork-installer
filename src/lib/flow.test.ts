import { describe, expect, it } from 'vitest'
import { STEP_ORDER } from './flow'

describe('flow step order', () => {
  it('runs the RCP key before the merged OpenCode step', () => {
    expect(STEP_ORDER).toEqual(['welcome', 'network', 'rcpKey', 'opencode', 'openwork', 'summary'])
  })
})
