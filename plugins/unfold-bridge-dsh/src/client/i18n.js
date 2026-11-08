import en from './locales/en.json'
import ptBR from './locales/pt-BR.json'

// The client's copy, in English and Brazilian Portuguese. DSH's own locale picks the language:
// any Portuguese locale reads pt-BR, anything else English.
export const DICTIONARIES = { en, 'pt-BR': ptBR }

export const languageOf = active => (typeof active === 'string' && active.toLowerCase().startsWith('pt') ? 'pt-BR' : 'en')

const fill = (text, params) => (params ? text.replace(/\{(\w+)\}/g, (whole, name) => (Object.hasOwn(params, name) ? String(params[name]) : whole)) : text)

// `t(key, params)`; `t.language` is the language it speaks, for dates.
export function translator(language) {
  const dictionary = DICTIONARIES[language] ?? en
  const t = (key, params) => fill(dictionary[key] ?? en[key] ?? key, params)
  t.language = DICTIONARIES[language] ? language : 'en'
  return t
}

// When a change was made, in the copy's language.
export const when = (at, t) => (Number.isFinite(at) ? new Intl.DateTimeFormat(t.language === 'pt-BR' ? 'pt-BR' : 'en-US', { dateStyle: 'short', timeStyle: 'short' }).format(at) : '')
