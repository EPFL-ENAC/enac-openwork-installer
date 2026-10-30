import { describe, expect, it } from 'vitest'
import { prereqMessages } from './prereq'

const translate = (key: string): string => `translated:${key}`

describe('linux prereq messages', () => {
  it('maps each missing library to its translated warning', () => {
    expect(prereqMessages(['webkit2gtk-4.1', 'libfuse2'], translate)).toEqual([
      'translated:linux.prereq.missingWebkit',
      'translated:linux.prereq.missingFuse',
    ])
  })

  it('returns no message when nothing is missing', () => {
    expect(prereqMessages([], translate)).toEqual([])
  })

  it('ignores unknown library ids', () => {
    expect(prereqMessages(['libfoo'], translate)).toEqual([])
  })
})
