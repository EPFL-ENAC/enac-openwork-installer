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
import { registerSecret } from '../lib/trace'

const SHORT_TOKEN = 'k9z'

const render = (detail: string, id: 'opencode' | 'openwork' = 'opencode'): Promise<string> => {
  const app = createSSRApp({ render: () => h(StepCard, { id, detail }) })
  app.use(
    createI18n({
      legacy: false,
      locale: 'en',
      messages: {
        en: {
          stepTitles: { opencode: 'OpenCode', openwork: 'OpenWork' },
          trace: { status: {} },
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
