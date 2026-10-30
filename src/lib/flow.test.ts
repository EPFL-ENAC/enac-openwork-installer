import { describe, expect, it } from 'vitest'
import {
  STEP_ORDER,
  isStepUnlocked,
  nextStep,
  recordResult,
  unlockedSteps,
  type FlowResults,
  type StepId,
  type StepResult,
} from './flow'

const pass = (extra: Partial<StepResult> = {}): StepResult => ({ status: 'passed', ...extra })
const skip = (extra: Partial<StepResult> = {}): StepResult => ({ status: 'skipped', ...extra })
const fail = (extra: Partial<StepResult> = {}): StepResult => ({ status: 'failed', ...extra })

const passThrough = (upto: number): FlowResults =>
  Object.fromEntries(STEP_ORDER.slice(0, upto).map((id) => [id, pass()])) as FlowResults

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

describe('gating unlock rules', () => {
  it('unlocks the first step always', () => {
    expect(isStepUnlocked('welcome', {})).toBe(true)
  })

  it('locks every later step when nothing is done', () => {
    const later = STEP_ORDER.slice(1) as StepId[]
    expect(later.map((id) => isStepUnlocked(id, {}))).toEqual(later.map(() => false))
  })

  it('unlocks only the step right after a passed one', () => {
    const results = passThrough(2)
    expect(isStepUnlocked('opencode', results)).toBe(true)
    expect(isStepUnlocked('rcpKey', results)).toBe(false)
    expect(isStepUnlocked('summary', results)).toBe(false)
  })

  it('unlocks the next step when the previous one is skipped', () => {
    const results: FlowResults = { welcome: pass(), network: pass(), opencode: skip({ version: '1.0.0' }) }
    expect(isStepUnlocked('rcpKey', results)).toBe(true)
    expect(isStepUnlocked('providerConfig', results)).toBe(false)
  })

  it('keeps the next step locked when the previous one failed', () => {
    const results: FlowResults = { welcome: pass(), network: fail() }
    expect(isStepUnlocked('opencode', results)).toBe(false)
  })

  it('unlocks the summary only after the whole chain', () => {
    expect(isStepUnlocked('summary', passThrough(6))).toBe(false)
    expect(isStepUnlocked('summary', passThrough(7))).toBe(true)
  })

  it('lists exactly the unlocked steps', () => {
    const results: FlowResults = { welcome: pass() }
    expect(unlockedSteps(results)).toEqual(['welcome', 'network'])
    expect(unlockedSteps(passThrough(8))).toEqual([...STEP_ORDER])
  })
})

describe('progression', () => {
  it('gives the next step in order', () => {
    expect(nextStep('welcome')).toBe('network')
    expect(nextStep('smokeTest')).toBe('openwork')
  })

  it('has no step after the summary', () => {
    expect(nextStep('summary')).toBeNull()
  })
})

describe('recordResult', () => {
  it('records a result and keeps other entries', () => {
    const before: FlowResults = { welcome: pass() }
    const after = recordResult(before, 'network', pass())

    expect(after.network).toEqual({ status: 'passed' })
    expect(after.welcome).toEqual({ status: 'passed' })
  })

  it('does not mutate the input', () => {
    const before: FlowResults = { welcome: pass() }
    recordResult(before, 'network', pass())
    expect(before.network).toBeUndefined()
  })

  it('keeps the payload fields of a step result', () => {
    const after = recordResult({}, 'opencode', skip({ version: '1.0.0', path: '/usr/bin/opencode' }))
    expect(after.opencode).toEqual({ status: 'skipped', version: '1.0.0', path: '/usr/bin/opencode' })
  })

  it('replaces an earlier result for the same step', () => {
    const after = recordResult(recordResult({}, 'network', fail()), 'network', pass())
    expect(after.network).toEqual({ status: 'passed' })
  })
})
