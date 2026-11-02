import { describe, expect, it, vi } from 'vitest'
import { createSSRApp, h } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { createI18n } from 'vue-i18n'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

import StepCard from './StepCard.vue'
import { registerSecret } from '../lib/trace'

const SHORT_TOKEN = 'k9z'

const render = (detail: string): Promise<string> => {
  const app = createSSRApp({ render: () => h(StepCard, { id: 'smokeTest', detail }) })
  app.use(
    createI18n({
      legacy: false,
      locale: 'en',
      messages: { en: { stepTitles: { smokeTest: 'Smoke test' }, trace: { status: {} } } },
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
