import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createSSRApp, h } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { createI18n } from 'vue-i18n'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))
vi.mock('vue-router', () => ({ useRouter: () => ({ replace: vi.fn() }) }))

const holder = vi.hoisted(() => ({
  store: {
    state: 'running',
    results: {},
    keyPrompt: null,
    failedStep: null,
    answerKeys: vi.fn(),
    retry: vi.fn(),
  },
}))

vi.mock('../stores/flow', async () => {
  const { ref, reactive } = await import('vue')
  holder.store = {
    ...holder.store,
    state: ref(holder.store.state) as unknown as string,
    results: reactive({}) as unknown as Record<string, never>,
    keyPrompt: ref(null) as unknown as null,
    failedStep: ref(null) as unknown as null,
  }
  return { useFlow: () => holder.store }
})

import RunView from './RunView.vue'
import en from '../i18n/en.json'
import { trace } from '../lib/trace'

const store = holder.store as unknown as {
  state: { value: string }
  results: Record<string, unknown>
  keyPrompt: { value: unknown }
}

const render = (): Promise<string> => {
  const app = createSSRApp({ render: () => h(RunView) })
  app.use(
    createI18n({
      legacy: false,
      locale: 'en',
      messages: { en },
      missingWarn: false,
      fallbackWarn: false,
    }),
  )
  return renderToString(app)
}

const count = (markup: string, needle: string): number => markup.split(needle).length - 1

describe('RunView progress bar', () => {
  beforeEach(() => {
    trace.reset()
  })

  it('counts the completed steps over the four installation steps', async () => {
    trace.setStepStatus('network', 'passed')
    trace.setStepStatus('rcpKey', 'passed')
    trace.setStepStatus('opencode', 'running')

    const markup = await render()

    expect(markup).toContain('role="progressbar"')
    expect(markup).toContain('aria-valuemin="0"')
    expect(markup).toContain('aria-valuemax="4"')
    expect(markup).toContain('aria-valuenow="2"')
    expect(markup).toContain('Step 3 of 4')
    expect(count(markup, 'run-progress-seg')).toBe(4)
    expect(count(markup, 'is-done')).toBe(2)
  })

  it('starts at the first step with nothing done', async () => {
    trace.setStepStatus('network', 'running')

    const markup = await render()

    expect(markup).toContain('aria-valuenow="0"')
    expect(markup).toContain('Step 1 of 4')
    expect(count(markup, 'is-done')).toBe(0)
  })

  it('counts a skipped step as done', async () => {
    trace.setStepStatus('network', 'passed')
    trace.setStepStatus('rcpKey', 'skipped')

    const markup = await render()

    expect(markup).toContain('aria-valuenow="2"')
  })
})

const source = async (): Promise<string> => {
  const specifier = 'node:fs'
  const fs = (await import(/* @vite-ignore */ specifier)) as {
    readFileSync: (url: URL, encoding: 'utf8') => string
  }
  return fs.readFileSync(new URL('./RunView.vue', import.meta.url), 'utf8')
}

const styleBlock = (css: string, selector: string): string => {
  const start = css.indexOf(selector)
  expect(start, `${selector} is styled`).toBeGreaterThan(-1)
  return css.slice(start, css.indexOf('}', start))
}

describe('RunView progress colours', () => {
  it('renders the whole bar solid red at every step', async () => {
    const css = await source()

    expect(styleBlock(css, '.run-progress {')).toContain('var(--epfl-red)')
    expect(css).not.toContain('var(--border-subtle)')
  })
})

describe('RunView paused step', () => {
  it('shows the waiting mark while the rcpKey step waits on the user', async () => {
    trace.reset()
    trace.setStepStatus('rcpKey', 'running')

    const markup = await render()

    expect(markup).toContain('Waiting for you')
    expect(markup).not.toContain('step-spin')
  })

  it('spins the active step that works on its own', async () => {
    trace.reset()
    trace.setStepStatus('opencode', 'running')

    const markup = await render()

    expect(markup).toContain('step-spin')
    expect(markup).not.toContain('Waiting for you')
  })
})
