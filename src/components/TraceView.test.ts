import { describe, expect, it, vi } from 'vitest'
import { createSSRApp, h } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { createI18n } from 'vue-i18n'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

import TraceView from './TraceView.vue'
import en from '../i18n/en.json'
import { formatClock, registerSecret, trace } from '../lib/trace'

const render = (): Promise<string> => {
  const app = createSSRApp({ render: () => h(TraceView) })
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

describe('TraceView side log', () => {
  it('renders the masked log lines with their clock and scope', async () => {
    trace.reset()
    registerSecret('k9z-secret')
    trace.log('network', 'Checking the portal with k9z-secret')

    const markup = await render()

    expect(markup).toContain(formatClock(trace.entries[0].ts))
    expect(markup).toContain('***')
    expect(markup).not.toContain('k9z-secret')
  })

  it('keeps the full step trace off the run screen', async () => {
    trace.reset()
    trace.setStepStatus('opencode', 'running')

    const markup = await render()

    expect(markup).not.toContain('trace-step')
    expect(markup).not.toContain(en.trace.status.running)
    expect(markup).not.toContain(en.stepTitles.opencode)
  })
})
