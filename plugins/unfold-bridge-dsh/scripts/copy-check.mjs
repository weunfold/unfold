// The copy check: every string the DSH client shows, in English and Brazilian Portuguese, is
// read against the claims this plugin must never make. The models come from the person's own
// DSH and the watch runs through it, so no line may promise local-only or offline work, or name
// a model provider. No line calls DSH a sandbox, and a click is a click (never tap, toque or
// toca). Unfold has no gender: Portuguese copy never puts an article before it or refers to it
// (or to the person) with a gendered word.
//
//   node scripts/copy-check.mjs                  checks src/client/locales/{en,pt-BR}.json
//   node scripts/copy-check.mjs <en> <pt-BR>     checks other locale files (a fixture)
//
// Exits 1 with one line per finding.
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const PLUGIN = dirname(dirname(fileURLToPath(import.meta.url)))
export const LOCALE_FILES = { en: join(PLUGIN, 'src/client/locales/en.json'), 'pt-BR': join(PLUGIN, 'src/client/locales/pt-BR.json') }

const BOTH = [
  ['names a model provider', /\b(DeepSeek|OpenAI|Anthropic|Claude|GPT|Gemini|Granite|Flash|Qwen|Llama)\b/i],
  ['calls DSH a sandbox', /\bsandbox(es|ed)?\b/i],
]
export const RULES = {
  en: [
    ...BOTH,
    ['promises local-only work', /\blocal(ly)?\b|\bon[- ]device\b/i],
    ['promises offline work', /\boffline\b|\bwithout (the )?internet\b/i],
    ['promises it stays on this machine', /\bon (your|this) (Mac|computer|machine|device)\b|\bnever leaves\b/i],
    ['genders Unfold', /\b(he|she|him|her|his|hers|himself|herself)\b/i],
    ['says tap, not click', /\btap(s|ped|ping)?\b/i],
  ],
  'pt-BR': [
    ...BOTH,
    ['promises local-only work', /\blocal(mente)?\b|\bno dispositivo\b/i],
    ['promises offline work', /\boffline\b|\bsem (a )?(internet|conexão com a internet)\b/i],
    ['promises it stays on this machine', /\b(no seu|neste|nesse) (Mac|computador|dispositivo)\b|\bnunca sai\b/i],
    ['puts an article before Unfold', /\b(o|a|os|as|do|da|dos|das|no|na|nos|nas|ao|à|aos|às|pelo|pela|num|numa|dum|duma)\s+Unfold\b/i],
    ['uses a gendered pronoun', /\b(ele|ela|eles|elas|dele|dela|deles|delas|nele|nela)\b/i],
    ['uses a gendered word for a person or for Unfold', /\b(conectad|desconectad|pront|ocupad|sozinh|obrigad|bem-vind|atent|cert|segur|parad|atualizad)[oa]s?\b/i],
    ['is European Portuguese', /\b(ecrã|utilizador|ficheiro|telemóvel|registo|equipa)\b/i],
    ['says toque, not clique', /\b(toque|toques|toca|tocar|tocou|toque-se)\b/i],
  ],
}

// Every string of a locale file, as [key, text]; nested objects are walked.
const strings = (value, prefix = '') => Object.entries(value).flatMap(([key, text]) =>
  typeof text === 'string' ? [[`${prefix}${key}`, text]] : text && typeof text === 'object' ? strings(text, `${prefix}${key}.`) : [[`${prefix}${key}`, text]])

export function checkCopy(files = LOCALE_FILES) {
  const findings = []
  const keys = {}
  for (const [lang, file] of Object.entries(files)) {
    let entries
    try {
      entries = strings(JSON.parse(readFileSync(file, 'utf8')))
    } catch (error) {
      findings.push(`${lang}: no locale file to check (${error.code ?? error.message})`)
      continue
    }
    if (entries.length === 0) findings.push(`${lang}: the locale file holds no strings`)
    keys[lang] = new Set(entries.map(([key]) => key))
    for (const [key, text] of entries) {
      if (typeof text !== 'string' || text.trim() === '') {
        findings.push(`${lang} ${key}: empty or not text`)
        continue
      }
      for (const [rule, pattern] of RULES[lang] ?? []) {
        const hit = text.match(pattern)
        if (hit) findings.push(`${lang} ${key}: ${rule} ("${hit[0]}" in "${text}")`)
      }
    }
  }
  const [a, b] = Object.keys(keys)
  if (a && b) {
    for (const key of keys[a]) if (!keys[b].has(key)) findings.push(`${b} ${key}: missing (present in ${a})`)
    for (const key of keys[b]) if (!keys[a].has(key)) findings.push(`${a} ${key}: missing (present in ${b})`)
  }
  return findings
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [en, pt] = process.argv.slice(2)
  const findings = checkCopy(en && pt ? { en, 'pt-BR': pt } : LOCALE_FILES)
  for (const line of findings) console.error(`copy-check: ${line}`)
  if (findings.length > 0) process.exit(1)
  console.log(`copy-check: ${Object.keys(LOCALE_FILES).join(' and ')} pass`)
}
