import { createI18n } from 'vue-i18n'
import zhCN from './locales/zh-CN'
import en from './locales/en'

export type LocaleKey = 'zh-CN' | 'en'
export type LocalePref = 'auto' | LocaleKey

const STORAGE_KEY = 'mirror-locale'
const SUPPORTED: LocaleKey[] = ['zh-CN', 'en']
const DEFAULT_LOCALE: LocaleKey = 'zh-CN'

export function detectSystemLocale(): LocaleKey {
  const langs = (navigator.languages?.length ? navigator.languages : [navigator.language]) ?? [DEFAULT_LOCALE]
  for (const raw of langs) {
    if (!raw) continue
    const lower = raw.toLowerCase()
    if (lower.startsWith('zh')) return 'zh-CN'
    if (lower.startsWith('en')) return 'en'
  }
  return DEFAULT_LOCALE
}

export function readPref(): LocalePref {
  const v = localStorage.getItem(STORAGE_KEY)
  if (v === 'auto' || v === 'zh-CN' || v === 'en') return v
  return DEFAULT_LOCALE
}

export function writePref(v: LocalePref) {
  localStorage.setItem(STORAGE_KEY, v)
}

export function resolveLocale(pref: LocalePref): LocaleKey {
  return pref === 'auto' ? detectSystemLocale() : pref
}

const initialPref = readPref()
const initialLocale = resolveLocale(initialPref)

export const i18n = createI18n({
  legacy: false,
  globalInjection: true,
  locale: initialLocale,
  fallbackLocale: DEFAULT_LOCALE,
  messages: { 'zh-CN': zhCN, en },
})

export const SUPPORTED_LOCALES = SUPPORTED
