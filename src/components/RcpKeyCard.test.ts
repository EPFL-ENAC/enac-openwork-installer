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
            create: { nameLabel: 'Key name', submit: 'Create the key', nameError: 'Use 1 to 15 characters: A-Z, a-z, 0-9, hyphen and underscore.' },
            existingKeys: { title: 'Or reuse an existing key' },
            pool: { basic: 'basic', premium: 'premium' },
            choice: {
              title: 'An OpenWork key already exists',
              text: 'Reuse the existing key or create a second key.',
              pasteLabel: 'Token of the existing key',
              paste: 'Use the existing key',
              newNameLabel: 'New key name',
              createNew: 'Create a second key',
            },
          },
        },
      },
      missingWarn: false,
      fallbackWarn: false,
    }),
  )
  return renderToString(app)
}

const NAME_ERROR = 'Use 1 to 15 characters: A-Z, a-z, 0-9, hyphen and underscore.'

const createButton = (markup: string): string => {
  const label = markup.indexOf('Create the key')
  if (label < 0) throw new Error('create button missing')
  return markup.slice(markup.lastIndexOf('<button', label), label)
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

describe('RcpKeyCard name validation', () => {
  it('caps the name field at 15 characters', async () => {
    const markup = await render(prompt())

    expect(markup).toContain('maxlength="15"')
  })

  it('rejects a name with a space and disables the create button', async () => {
    const markup = await render(prompt({ suggestedKeyName: 'Bad Name' }))

    expect(markup).toContain(NAME_ERROR)
    expect(createButton(markup)).toContain('disabled')
  })

  it('accepts a name with a hyphen and an underscore', async () => {
    const markup = await render(prompt({ suggestedKeyName: 'OpenWork_2-a' }))

    expect(markup).toContain('value="OpenWork_2-a"')
    expect(markup).not.toContain(NAME_ERROR)
    expect(createButton(markup)).not.toContain('disabled')
  })
})

describe('RcpKeyCard create failure', () => {
  it('shows the failure and keeps the create form with the key list', async () => {
    const markup = await render(prompt({ error: 'A key with this name already exists.' }))

    expect(markup).toContain('role="alert"')
    expect(markup).toContain('A key with this name already exists.')
    expect(markup).toContain('value="OpenWork"')
    expect(markup).toContain('Create the key')
    expect(markup).toContain('Or reuse an existing key')
    expect(markup).toContain('AiReviewer')
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

  it('renders the plain create form for an explicit new name', async () => {
    const markup = await render(prompt({ branch: 'fresh', showExistingKeys: false, suggestedKeyName: 'MyKey' }))

    expect(markup).toContain('value="MyKey"')
    expect(markup).toContain('Create the key')
    expect(markup).not.toContain('An OpenWork key already exists')
    expect(markup).not.toContain('Token of the existing key')
    expect(markup).not.toContain('Create a second key')
    expect(markup).not.toContain('Or reuse an existing key')
  })

  it('keeps the existing-key form on the existing branch', async () => {
    const markup = await render(prompt({ branch: 'existing', showExistingKeys: false }))

    expect(markup).toContain('An OpenWork key already exists')
    expect(markup).toContain('Token of the existing key')
    expect(markup).toContain('Create a second key')
  })
})
