import { describe, expect, it } from 'vitest'
import en from './en.json'
import fr from './fr.json'

const flatten = (value: unknown, prefix = ''): string[] =>
  Object.entries(value as Record<string, unknown>).flatMap(([key, child]) => {
    const path = prefix ? `${prefix}.${key}` : key
    return child !== null && typeof child === 'object'
      ? flatten(child, path)
      : [path]
  })

describe('i18n messages', () => {
  it('has the same keys in en and fr', () => {
    expect(flatten(fr).sort()).toEqual(flatten(en).sort())
  })
})
