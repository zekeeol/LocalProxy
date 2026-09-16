import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import en from './locales/en';
import zh from './locales/zh';
import ja from './locales/ja';
import ko from './locales/ko';

export const LANGUAGES = [
  { code: 'en', label: 'English' },
  { code: 'zh', label: '中文' },
  { code: 'ja', label: '日本語' },
  { code: 'ko', label: '한국어' },
] as const;

export type LanguageCode = (typeof LANGUAGES)[number]['code'];

const STORAGE_KEY = 'localproxy.language';

function isLanguageCode(value: string): value is LanguageCode {
  return LANGUAGES.some((language) => language.code === value);
}

function normalize(tag: string): LanguageCode {
  const lower = tag.toLowerCase();
  if (lower.startsWith('zh')) return 'zh';
  if (lower.startsWith('ja')) return 'ja';
  if (lower.startsWith('ko')) return 'ko';
  return 'en';
}

function detectLanguage(): LanguageCode {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    if (stored && isLanguageCode(stored)) return stored;
  } catch {
    /* localStorage unavailable */
  }
  const candidates = [...(navigator.languages ?? []), navigator.language].filter(Boolean);
  for (const tag of candidates) {
    const code = normalize(tag);
    if (code !== 'en') return code;
  }
  return 'en';
}

export function changeLanguage(code: LanguageCode): void {
  void i18n.changeLanguage(code);
  try {
    window.localStorage.setItem(STORAGE_KEY, code);
  } catch {
    /* localStorage unavailable */
  }
  document.documentElement.lang = code === 'zh' ? 'zh-CN' : code;
}

document.documentElement.lang = detectLanguage() === 'zh' ? 'zh-CN' : detectLanguage();

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    zh: { translation: zh },
    ja: { translation: ja },
    ko: { translation: ko },
  },
  lng: detectLanguage(),
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
  returnEmptyString: false,
});

export default i18n;
