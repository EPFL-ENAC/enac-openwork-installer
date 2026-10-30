import { createI18n } from 'vue-i18n'
import en from './en.json'
import fr from './fr.json'

export const messages = { en, fr }
export type Locale = keyof typeof messages
const locales: Locale[] = ['en', 'fr']

const detectLocale = (): Locale => {
  const language = navigator.language.slice(0, 2).toLowerCase()
  return locales.includes(language as Locale) ? (language as Locale) : 'en'
}

export const i18n = createI18n({
  legacy: false,
  locale: detectLocale(),
  fallbackLocale: 'en',
  messages,
})
