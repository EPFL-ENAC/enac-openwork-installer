import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createSSRApp, h } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { createI18n } from 'vue-i18n'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const { results } = vi.hoisted(() => ({
  results: {} as Record<
    string,
    { status: string; version?: string; keyName?: string; ok?: boolean; path?: string; configPath?: string }
  >,
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
  results.openwork = {
    status: 'passed',
    version: '0.6.0',
    path: '/opt/openwork',
    configPath: '~/.config/opencode/opencode.json',
    ok: true,
  }
})

describe('SummaryScreen list', () => {
  it('lists the three main steps in order, then the launch button', async () => {
    const markup = await render()

    const labels = [en.stepTitles.network, en.summary.keyName, en.summary.openwork]
    for (const label of labels) expect(markup).toContain(label)

    const positions = labels.map((label) => markup.indexOf(label))
    expect(positions).toEqual([...positions].sort((a, b) => a - b))

    expect(markup).toContain('0.6.0')
    expect(markup).toContain('openwork-live')
    expect(markup).toContain(en.summary.launch)
  })

  it('lists the shared OpenCode config path before the OpenWork path', async () => {
    const markup = await render()

    expect(markup).toContain(en.summary.opencode)
    expect(markup).toContain('~/.config/opencode/opencode.json')
    expect(markup).toContain('/opt/openwork')
    expect(markup.indexOf('~/.config/opencode/opencode.json')).toBeLessThan(
      markup.indexOf('/opt/openwork'),
    )
  })

  it('marks a run whose engine check was skipped as completed with a warning', async () => {
    results.openwork = { status: 'warning', version: '0.6.0', path: '/opt/openwork', ok: false }

    const markup = await render()

    expect(markup).toContain(en.summary.smokeSkipped)
    expect(markup).toContain(en.summary.launch)
  })

  it('shows no warning on a clean run', async () => {
    expect(await render()).not.toContain(en.summary.smokeSkipped)
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
