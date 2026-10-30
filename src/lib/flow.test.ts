import { describe, expect, it } from 'vitest'
import { STEP_ORDER } from './flow'

describe('flow step order', () => {
  it('has the eight install-flow steps in order', () => {
    expect(STEP_ORDER).toEqual([
      'welcome',
      'network',
      'opencode',
      'rcpKey',
      'providerConfig',
      'smokeTest',
      'openwork',
      'summary',
    ])
  })
})
