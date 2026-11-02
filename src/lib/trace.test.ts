import { beforeEach, describe, expect, it } from 'vitest'
import { formatClock, maskSecrets, registerSecret, trace, type TraceStepStatus } from './trace'

const SECRET_A = 'sk-alpha-0123456789'
const SECRET_B = 'rcp-beta-9876543210'
const SHORT_SECRET = 'k9z'

beforeEach(() => {
  trace.reset()
})

describe('trace store', () => {
  it('appends entries in order with timestamps', () => {
    const before = Date.now()
    trace.log('flow', 'start')
    trace.log('network', 'checking')

    expect(trace.entries.map((entry) => [entry.scope, entry.message])).toEqual([
      ['flow', 'start'],
      ['network', 'checking'],
    ])
    expect(trace.entries[0].ts).toBeGreaterThanOrEqual(before)
    expect(trace.entries[1].ts).toBeGreaterThanOrEqual(trace.entries[0].ts)
  })

  it('starts every step pending and updates one status at a time', () => {
    expect(Object.values(trace.stepStatuses).every((status) => status === 'pending')).toBe(true)

    const transitions: TraceStepStatus[] = ['running', 'passed']
    for (const status of transitions) {
      trace.setStepStatus('network', status)
      expect(trace.stepStatuses.network).toBe(status)
    }
    expect(trace.stepStatuses.opencode).toBe('pending')
  })

  it('resets entries and statuses', () => {
    trace.log('flow', 'start')
    trace.setStepStatus('network', 'failed')

    trace.reset()

    expect(trace.entries).toEqual([])
    expect(trace.stepStatuses.network).toBe('pending')
  })
})

describe('secret masking', () => {
  it('masks a registered secret everywhere it appears', () => {
    registerSecret(SECRET_A)
    trace.log('rcpKey', `token ${SECRET_A} and again ${SECRET_A}`)

    expect(trace.entries[0].message).toBe('token *** and again ***')
  })

  it('leaves text without a registered secret untouched', () => {
    expect(maskSecrets('no secret in this line')).toBe('no secret in this line')
  })

  it('masks several registered secrets', () => {
    registerSecret(SECRET_A)
    registerSecret(SECRET_B)
    trace.log('rcpKey', `${SECRET_A} then ${SECRET_B}`)

    expect(trace.entries[0].message).toBe('*** then ***')
  })

  it('masks a secret shorter than eight characters', () => {
    registerSecret(SHORT_SECRET)
    trace.log('smokeTest', `the request failed for ${SHORT_SECRET}`)

    expect(trace.entries[0].message).toBe('the request failed for ***')
  })

  it('ignores an empty value', () => {
    registerSecret('')

    expect(maskSecrets('nothing to mask here')).toBe('nothing to mask here')
  })

  it('never stores a secret in the entry', () => {
    registerSecret(SECRET_B)
    trace.log('rcpKey', `token ${SECRET_B}`)

    expect(JSON.stringify(trace.entries)).not.toContain(SECRET_B)
  })
})

describe('trace clock', () => {
  it('formats a timestamp as HH:MM:SS', () => {
    const ts = new Date(2025, 0, 2, 3, 4, 5).getTime()

    expect(formatClock(ts)).toBe('03:04:05')
  })
})
