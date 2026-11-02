import { describe, expect, it, vi } from 'vitest'
import { createSSRApp, h } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { createI18n } from 'vue-i18n'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const store = vi.hoisted(() => ({
  results: {} as Record<string, { status?: string; kind?: string; detail?: string }>,
  state: { value: 'failed' },
  failedStep: { value: null as string | null },
  retry: () => Promise.resolve(),
}))

vi.mock('../stores/flow', () => ({ useFlow: () => store }))

import StepCard from './StepCard.vue'
import { registerSecret, trace } from '../lib/trace'

const SHORT_TOKEN = 'k9z'

const render = (
  detail: string,
  id: 'opencode' | 'openwork' | 'rcpKey' = 'opencode',
  paused = false,
): Promise<string> => {
  const app = createSSRApp({ render: () => h(StepCard, { id, detail, paused }) })
  app.use(
    createI18n({
      legacy: false,
      locale: 'en',
      messages: {
        en: {
          stepTitles: { opencode: 'OpenCode', openwork: 'OpenWork', rcpKey: 'RCP key' },
          trace: { status: { pending: 'Pending', running: 'Running' } },
          progress: { waiting: 'Waiting for you' },
          run: { error: { generic: 'Something went wrong: {message}' } },
        },
      },
      missingWarn: false,
      fallbackWarn: false,
    }),
  )
  return renderToString(app)
}

describe('StepCard detail', () => {
  it('masks a registered token in the detail line', async () => {
    registerSecret(SHORT_TOKEN)

    const markup = await render(`RCP key ${SHORT_TOKEN} is ready`)

    expect(markup).toContain('***')
    expect(markup).not.toContain(SHORT_TOKEN)
  })

  it('keeps a detail without a token readable', async () => {
    expect(await render('OpenCode 1.2.3')).toContain('OpenCode 1.2.3')
  })
})

describe('StepCard activity mark', () => {
  it('shows the spinner and aria-busy on the active card', async () => {
    trace.reset()
    trace.setStepStatus('opencode', 'running')

    const markup = await render('')

    expect(markup).toContain('step-spin')
    expect(markup).toContain('aria-busy="true"')
  })

  it('keeps a pending card static', async () => {
    trace.reset()

    const markup = await render('')

    expect(markup).not.toContain('step-spin')
    expect(markup).not.toContain('aria-busy')
  })

  it('keeps a passed card static', async () => {
    trace.reset()
    trace.setStepStatus('opencode', 'passed')

    const markup = await render('')

    expect(markup).not.toContain('step-spin')
    expect(markup).not.toContain('aria-busy')
  })

  it('stops the spinner and shows the waiting mark while paused', async () => {
    trace.reset()
    trace.setStepStatus('rcpKey', 'running')

    const markup = await render('', 'rcpKey', true)

    expect(markup).not.toContain('step-spin')
    expect(markup).toContain('step-mark-paused')
    expect(markup).toContain('Waiting for you')
    expect(markup).not.toContain('aria-busy')
  })

  it('holds a static spinner under reduced motion', async () => {
    const specifier = 'node:fs'
    const fs = (await import(/* @vite-ignore */ specifier)) as {
      readFileSync: (url: URL, encoding: 'utf8') => string
    }
    const source = fs.readFileSync(new URL('./StepCard.vue', import.meta.url), 'utf8')
    const start = source.indexOf('prefers-reduced-motion')
    const block = source.slice(start, source.indexOf('}', start) + 1)

    expect(start).toBeGreaterThan(-1)
    expect(block).toContain('.step-spin')
    expect(block).toContain('animation: none')
  })
})

const source = async (): Promise<string> => {
  const specifier = 'node:fs'
  const fs = (await import(/* @vite-ignore */ specifier)) as {
    readFileSync: (url: URL, encoding: 'utf8') => string
  }
  return fs.readFileSync(new URL('./StepCard.vue', import.meta.url), 'utf8')
}

const styleBlock = (css: string, selector: string): string => {
  const start = css.indexOf(selector)
  expect(start, `${selector} is styled`).toBeGreaterThan(-1)
  return css.slice(start, css.indexOf('}', start))
}

describe('StepCard status palette', () => {
  it('marks a passed card with a check mark', async () => {
    trace.reset()
    trace.setStepStatus('opencode', 'passed')

    expect(await render('')).toContain('card-status-check')
  })

  it('shows no check mark on a pending card', async () => {
    trace.reset()

    expect(await render('')).not.toContain('card-status-check')
  })

  it('accents the running card in the primary red', async () => {
    const css = await source()

    expect(styleBlock(css, '.step-card-running')).toContain('var(--epfl-red)')
    expect(styleBlock(css, '.step-card-running')).toContain('border-left-width: 2px')
    expect(styleBlock(css, '.step-card-running .card-title')).toContain('var(--epfl-red)')
    expect(styleBlock(css, '.step-spin')).toContain('var(--epfl-red)')
  })

  it('paints pass grey and fail red, with no other status colour', async () => {
    const css = await source()

    expect(css).not.toMatch(/--(warning|success|info)/)
    expect(styleBlock(css, '.card-status-passed')).toContain('var(--fg-muted)')
    expect(styleBlock(css, '.card-status-failed')).toContain('var(--primary)')
  })
})

describe('StepCard error block', () => {
  it('shows no error block when the failed step has no result left', async () => {
    store.failedStep.value = 'openwork'
    store.results = {}

    const markup = await render('', 'openwork')

    expect(markup).not.toContain('Something went wrong')
  })

  it('shows no error block for a whitespace-only detail', async () => {
    store.failedStep.value = 'openwork'
    store.results = { openwork: { status: 'failed', kind: 'io', detail: '   ' } }

    const markup = await render('', 'openwork')

    expect(markup).not.toContain('Something went wrong')
  })

  it('shows a real error with its message', async () => {
    store.failedStep.value = 'openwork'
    store.results = {
      openwork: { status: 'failed', kind: 'unexpected', detail: 'the database is locked' },
    }

    const markup = await render('', 'openwork')

    expect(markup).toContain('Something went wrong')
    expect(markup).toContain('the database is locked')
  })
})
