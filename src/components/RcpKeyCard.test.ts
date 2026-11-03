import { describe, expect, it, vi } from 'vitest'
import { createSSRApp, h } from 'vue'
import { renderToString } from 'vue/server-renderer'
import { createI18n } from 'vue-i18n'

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }))

import RcpKeyCard from './RcpKeyCard.vue'
import type { KeyPrompt } from '../lib/runners'

const prompt = (over: Partial<KeyPrompt> = {}): KeyPrompt => ({
  branch: 'fresh',
  unit: 'ENAC',
  showUnitPicker: false,
  showExistingKeys: true,
  suggestedKeyName: 'OpenWork',
  units: ['ENAC'],
  availableKeys: [
    { id: 1462, name: 'OpenWork', pool: 'BASIC' },
    { id: 834, name: 'AiReviewer', pool: 'PREMIUM' },
  ],
  manual: false,
  ...over,
})

const render = (value: KeyPrompt): Promise<string> => {
  const app = createSSRApp({ render: () => h(RcpKeyCard, { prompt: value }) })
  app.use(
    createI18n({
      legacy: false,
      locale: 'en',
      messages: {
        en: {
          rcpKey: {
            unit: { label: 'Unit', placeholder: 'Choose your unit' },
            create: { nameLabel: 'Key name', submit: 'Create the key' },
            existingKeys: { title: 'Or reuse an existing key' },
            pool: { basic: 'basic', premium: 'premium' },
          },
        },
      },
      missingWarn: false,
      fallbackWarn: false,
    }),
  )
  return renderToString(app)
}

describe('RcpKeyCard unit picker', () => {
  it('shows the placeholder and every unit when several units exist', async () => {
    const markup = await render(
      prompt({ showUnitPicker: true, unit: null, units: ['ENAC-IT', 'LSMS'] }),
    )

    expect(markup).toContain('Choose your unit')
    expect(markup).toContain('ENAC-IT')
    expect(markup).toContain('LSMS')
  })

  it('renders one entry per distinct unit', async () => {
    const markup = await render(prompt({ showUnitPicker: true, unit: null, units: ['ENAC-IT', 'ENAC-IT'] }))

    expect(markup.match(/>ENAC-IT<\/option>/g)).toHaveLength(1)
  })

  it('shows no picker when the prompt carries a single unit', async () => {
    const markup = await render(prompt())

    expect(markup).not.toContain('Choose your unit')
  })
})

describe('RcpKeyCard key list', () => {
  it('lists each selectable key with its pool label', async () => {
    const markup = await render(prompt())

    expect(markup).toContain('Or reuse an existing key')
    expect(markup).toContain('AiReviewer')
    expect(markup).toContain('premium')
    expect(markup).toContain('OpenWork')
    expect(markup).toContain('basic')
    expect(markup).toContain('Create the key')
  })

  it('shows no key list on a branch that does not offer one', async () => {
    const markup = await render(prompt({ branch: 'existing', showExistingKeys: false }))

    expect(markup).not.toContain('AiReviewer')
    expect(markup).not.toContain('Or reuse an existing key')
  })
})
