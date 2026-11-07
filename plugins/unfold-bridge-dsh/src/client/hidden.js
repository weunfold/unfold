// Characters a person can't see in a draft or in Unfold's advice: agentd's own ignorable set
// (default-ignorables, bidi controls, variation selectors, tags) plus C0 and C1 controls other
// than \n, \t and \r. The text is shown as held; these only raise a warning and hold Send back.
const RANGES = [
  [0x0000, 0x0008], [0x000B, 0x000C], [0x000E, 0x001F], [0x007F, 0x009F],
  [0x00AD, 0x00AD], [0x034F, 0x034F], [0x061C, 0x061C], [0x115F, 0x1160], [0x17B4, 0x17B5],
  [0x180B, 0x180F], [0x200B, 0x200F], [0x202A, 0x202E], [0x2060, 0x206F], [0x3164, 0x3164],
  [0xFE00, 0xFE0F], [0xFEFF, 0xFEFF], [0xFFA0, 0xFFA0], [0xFFF0, 0xFFF8],
  [0x1BCA0, 0x1BCA3], [0x1D173, 0x1D17A], [0xE0000, 0xE0FFF],
]
const ZWJ = 0x200D
const VS16 = 0xFE0F
const EMOJI = /\p{Extended_Pictographic}/u
const SKIN_TONE = cp => cp >= 0x1F3FB && cp <= 0x1F3FF

const listed = cp => RANGES.some(([low, high]) => cp >= low && cp <= high)
const emoji = cp => cp !== undefined && EMOJI.test(String.fromCodePoint(cp))

// The one exemption, shared with agentd's draft check: a U+FE0F right after an
// Extended_Pictographic character, and a U+200D between two of them, where the first may carry
// its U+FE0F or a skin-tone modifier before the U+200D. Everything else in the set counts.
const presentation = (points, i) => points[i] === VS16 && emoji(points[i - 1])
function visibleJoin(points, i) {
  if (presentation(points, i)) return true
  if (points[i] !== ZWJ || !emoji(points[i + 1])) return false
  return emoji(points[i - 1]) || presentation(points, i - 1) || (SKIN_TONE(points[i - 1]) && emoji(points[i - 2]))
}

// The hidden code points in `text`, in order.
export function hiddenIn(text) {
  if (typeof text !== 'string') return []
  const points = Array.from(text, char => char.codePointAt(0))
  return points.filter((cp, i) => listed(cp) && !visibleJoin(points, i))
}

export const codeName = cp => `U+${cp.toString(16).toUpperCase().padStart(4, '0')}`

// The warning line: how many, and which (each named once, the first dozen).
export function hiddenWarning(points, t) {
  const names = [...new Set(points)].map(codeName)
  const list = names.length > 12 ? `${names.slice(0, 12).join(', ')}, …` : names.join(', ')
  return points.length === 1 ? t('hidden.one', { list }) : t('hidden.many', { count: points.length, list })
}
