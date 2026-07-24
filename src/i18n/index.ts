import { createI18n } from 'vue-i18n'
import zhCN from './locales/zh-CN'
import enUS from './locales/en-US'

export type AppLocale = 'zh-CN' | 'en-US'

const STORAGE_KEY = 'app-locale'

function detectLocale(): AppLocale {
  // 1. localStorage 优先
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    if (saved === 'zh-CN' || saved === 'en-US') return saved
  } catch (e) {
    // 访问受限（如 Tauri 某些环境），忽略
  }
  // 2. 浏览器语言检测
  const navLang = (navigator.language || 'zh-CN').toLowerCase()
  return navLang.startsWith('zh') ? 'zh-CN' : 'en-US'
}

export function setLocale(locale: AppLocale) {
  i18n.global.locale.value = locale
  try {
    localStorage.setItem(STORAGE_KEY, locale)
  } catch (e) {
    // 忽略
  }
  // 同步设置 HTML lang 属性（便于辅助技术识别）
  document.documentElement.lang = locale === 'zh-CN' ? 'zh-CN' : 'en'
}

export function toggleLocale() {
  const next: AppLocale = i18n.global.locale.value === 'zh-CN' ? 'en-US' : 'zh-CN'
  setLocale(next)
}

const i18n = createI18n({
  legacy: false,
  locale: detectLocale(),
  fallbackLocale: 'zh-CN',
  messages: {
    'zh-CN': zhCN,
    'en-US': enUS,
  },
})

export default i18n
