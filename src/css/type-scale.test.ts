import { describe, expect, it } from 'vitest'

const read = async (path: string): Promise<string> => {
  const specifier = 'node:fs'
  const fs = (await import(/* @vite-ignore */ specifier)) as {
    readFileSync: (url: URL, encoding: 'utf8') => string
  }
  return fs.readFileSync(new URL(path, import.meta.url), 'utf8')
}

const tokensCss = await read('./epfl-tokens.css')
const formCss = await read('./epfl/atoms/form.css')
const buttonCss = await read('./epfl/button.css')
const cardCss = await read('./epfl/card.css')
const layoutCss = await read('./epfl/layout.css')

const ROOT_PX = 16
const SCALE = 0.85

const target = (originalPx: number): number => Math.round(originalPx * SCALE) / ROOT_PX

const fontSizeOf = (css: string, selector: string): string => {
  const start = css.indexOf(selector)
  expect(start, `selector ${selector} is present`).toBeGreaterThanOrEqual(0)
  const block = css.slice(start, css.indexOf('}', start))
  const match = block.match(/font-size:\s*([^;]+);/)
  expect(match, `selector ${selector} declares a font-size`).not.toBeNull()
  return match![1].trim()
}

const tokenOf = (name: string): string => {
  const match = tokensCss.match(new RegExp(`--${name}:\\s*([^;]+);`))
  expect(match, `token --${name} is defined`).not.toBeNull()
  return match![1].trim()
}

const mediaSizes = (maxPx: string): Record<string, number> => {
  const start = tokensCss.indexOf(`@media (max-width: ${maxPx}px)`)
  expect(start, `breakpoint ${maxPx}px is present`).toBeGreaterThanOrEqual(0)
  const block = tokensCss.slice(start, tokensCss.indexOf('\n}', start))
  const sizes: Record<string, number> = {}
  for (const match of block.matchAll(/(h\d)[^{}]*\{[^{}]*font-size:\s*([\d.]+)rem/g)) {
    sizes[match[1]] = Number.parseFloat(match[2])
  }
  return sizes
}

describe('EPFL type scale at 85% of the original size', () => {
  it('scales every type token to round(0.85 x original)', () => {
    const originals: Record<string, number> = {
      h1: 48,
      h2: 38,
      h3: 28,
      h4: 24,
      h5: 20,
      h6: 18,
      lead: 18,
      body: 16,
      sm: 14,
      xs: 0.778 * ROOT_PX,
    }
    for (const [name, originalPx] of Object.entries(originals)) {
      expect(Number.parseFloat(tokenOf(`fs-${name}`)), `--fs-${name}`).toBeCloseTo(
        target(originalPx),
        6,
      )
    }
  })

  it('scales the body and lead clamps to round(0.85 x original)', () => {
    expect(fontSizeOf(tokensCss, 'body {')).toBe(
      `clamp(${target(16)}rem, calc(${target(15)}rem + ${0.25 * SCALE}vw), ${target(18)}rem)`,
    )
    expect(fontSizeOf(tokensCss, '.lead {')).toBe(
      `clamp(${target(16)}rem, calc(${target(13)}rem + ${0.75 * SCALE}vw), ${target(22)}rem)`,
    )
  })

  it('scales the responsive headings to round(0.85 x original)', () => {
    expect(mediaSizes('991.98')).toEqual({
      h1: target(46),
      h2: target(36),
      h3: target(26),
      h4: target(22),
    })
    expect(mediaSizes('767.98')).toEqual({ h1: target(40) })
    expect(mediaSizes('575.98')).toEqual({
      h1: target(32),
      h2: target(26),
      h3: target(22),
      h4: target(20),
      h5: target(16),
      h6: target(15),
    })
  })

  it('keeps the heading order at the narrowest breakpoint', () => {
    const narrow = mediaSizes('575.98')
    expect(narrow.h6).toBeLessThan(narrow.h5)
    expect(narrow.h5).toBeLessThan(narrow.h4)
    expect(narrow.h4).toBeLessThan(narrow.h3)
    expect(narrow.h3).toBeLessThan(narrow.h2)
    expect(narrow.h2).toBeLessThan(narrow.h1)
  })

  it('scales the raw font sizes to round(0.85 x original)', () => {
    expect(fontSizeOf(tokensCss, 'blockquote, .blockquote {')).toBe(`${target(24)}rem`)
    expect(fontSizeOf(layoutCss, '.epfl-eyebrow {')).toBe(`${target(12)}rem`)
    expect(fontSizeOf(layoutCss, '.epfl-breadcrumb {')).toBe(`${target(13)}rem`)
    expect(fontSizeOf(cardCss, '.epfl-card-text ')).toBe(`${target(0.95 * ROOT_PX)}rem`)
    expect(fontSizeOf(buttonCss, '.epfl-btn {')).toBe(`${target(16)}rem`)
    expect(fontSizeOf(formCss, '.epfl-input,')).toBe(`${target(16)}rem`)
  })
})
