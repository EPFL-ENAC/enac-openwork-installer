import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createSSRApp, h } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { createI18n } from 'vue-i18n'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const { results } = vi.hoisted(() => ({
  results: {} as Record<string, { status: string; version?: string; keyName?: string; ok?: boolean }>,
}))

vi.mock('../../stores/flow', () => ({ useFlow: () => ({ results }) }))

import SummaryScreen from './SummaryScreen.vue'
import en from '../../i18n/en.json'

const render = (): Promise<string> => {
  const app = createSSRApp({ render: () => h(SummaryScreen) })
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

beforeEach(() => {
  for (const id of Object.keys(results)) delete results[id]
  results.network = { status: 'passed' }
  results.rcpKey = { status: 'passed', keyName: 'openwork-live' }
  results.opencode = { status: 'passed', version: '1.2.3', ok: true }
  results.openwork = { status: 'passed', version: '0.6.0' }
})

describe('SummaryScreen list', () => {
  it('lists the four main steps in order, then the launch button', async () => {
    const markup = await render()

    const labels = [
      en.stepTitles.network,
      en.summary.keyName,
      en.summary.opencode,
      en.summary.openwork,
    ]
    for (const label of labels) expect(markup).toContain(label)

    const positions = labels.map((label) => markup.indexOf(label))
    expect(positions).toEqual([...positions].sort((a, b) => a - b))

    expect(markup).toContain('1.2.3')
    expect(markup).toContain('openwork-live')
    expect(markup).toContain(en.summary.launch)
  })

  it('puts the close button after the launch and documentation buttons', async () => {
    const markup = await render()

    const close = markup.indexOf(`>${en.summary.close}<`)
    expect(close).toBeGreaterThan(-1)
    expect(close).toBeGreaterThan(markup.indexOf(`>${en.summary.launch}<`))
    expect(close).toBeGreaterThan(markup.indexOf(`>${en.summary.docsLink}<`))
  })

  it('keeps the smoke test line and the intro heading off the page', async () => {
    const markup = await render()

    expect(markup).not.toContain('Smoke test')
    expect(markup).not.toContain('Here is what this installer did')
  })
})
