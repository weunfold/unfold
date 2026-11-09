// The settings plugin's credential masking, copied rather than imported: each Unfold plugin installs
// on its own. Same contract as src/window.js `redactText` and `redact`; keep them in step.
// Secret values never leave this plugin. Everything served passes through `redact`: a value
// under a secret-looking key is replaced, whatever it is, and a string that carries a bearer
// credential, a Google API key, a standalone sk- key or a KEY=value assignment has that part replaced. Hashes and
// ids (artifact_id, *_sha256, boot ids) are not secrets and are kept: no length or hex rule.
export const REDACTED = '[redacted]'

const SECRET_KEY = /(token|secret|passw(or)?d|passphrase|(?:^|[_-])pass$|api[_-]?key|private[_-]?key|authorization|bearer|cookie|credential)/i
// Invisible characters inside a credential name (`pass\u200bword: x`, `{"pass\u0000word": …}`): NUL and
// the other C0 controls but tab and line ends, DEL, and the Unicode format characters (Cf: zero-width
// space and joiners, bidi marks, word joiner, BOM, soft hyphen), raw or as a JSON \u escape. A name
// read with them is the same name. `redact` tests a key without them and keeps the key as written.
// `nameText` (the first step of `redactText`) drops them from a run of name characters that a
// separator ends (`:`, `=`, a quoted or escaped JSON key's `":` or `\":`) and that holds a credential
// stem, and right after that separator, so `pass\u200bword: x` serves as `password: [redacted]`.
// Nothing else changes: an emoji's joiner, a ZWNJ in Persian text, and an invisible inside a word
// with no separator after it, or in a key with no credential stem, stay. A run starts only after a
// character that is not a name character, an invisible or a backslash (a \n \r \t escape excepted),
// so each run is read once.
const INVISIBLE_GATE = /[\0-\x08\x0e-\x1f\x7f\p{Cf}\u2065]|\\u/u
const INVISIBLE = /[\0-\x08\x0e-\x1f\x7f\p{Cf}\u2065]|\\{1,2}u(?:00(?:0[0-8EFef]|1[0-9A-Fa-f])|007[Ff]|00[Aa][Dd]|061[Cc]|180[Ee]|200[B-Fb-f]|202[A-Ea-e]|206[0-9A-Fa-f]|[Ff][Ee][Ff][Ff])/gu
const INVISIBLE_NAME = /(?<![A-Za-z0-9_-]|[\0-\x08\x0e-\x1f\x7f\p{Cf}\u2065]|\\{1,2}u(?:00(?:0[0-8EFef]|1[0-9A-Fa-f])|007[Ff]|00[Aa][Dd]|061[Cc]|180[Ee]|200[B-Fb-f]|202[A-Ea-e]|206[0-9A-Fa-f]|[Ff][Ee][Ff][Ff]))(?:(?<!\\)|(?=[nrt]))((?:[A-Za-z0-9_-]|[\0-\x08\x0e-\x1f\x7f\p{Cf}\u2065]|\\{1,2}u(?:00(?:0[0-8EFef]|1[0-9A-Fa-f])|007[Ff]|00[Aa][Dd]|061[Cc]|180[Ee]|200[B-Fb-f]|202[A-Ea-e]|206[0-9A-Fa-f]|[Ff][Ee][Ff][Ff]))+)((?:\\?["'])?[ \t]*[:=][ \t]*)((?:[\0-\x08\x0e-\x1f\x7f\p{Cf}\u2065]|\\{1,2}u(?:00(?:0[0-8EFef]|1[0-9A-Fa-f])|007[Ff]|00[Aa][Dd]|061[Cc]|180[Ee]|200[B-Fb-f]|202[A-Ea-e]|206[0-9A-Fa-f]|[Ff][Ee][Ff][Ff]))*)/gu
const NAME_STEM = /pass|secret|token|key|authorization/i
// The name on a YAML explicit key line: `? name`, quoted or not, after a list item's `- ` and any
// anchors or tags, then spaces or a comment to its line end (or a colon and its value). A quoted
// name is read whole, a colon inside it included. Its `:` line is what makes it a key line; the
// name is read without its invisible characters here, before the rules look for it, so the value
// under it is found. Only the name changes; a line with no credential stem stays.
const INVISIBLE_EXPLICIT = /(^|\r\n|\r(?!\n)|\n)([ \t]*(?:-[ \t]+)?\?[ \t][^\r\n#]*?["']?)((?:[A-Za-z0-9_:-]|[\0-\x08\x0e-\x1f\x7f\p{Cf}\u2065]|\\{1,2}u(?:00(?:0[0-8EFef]|1[0-9A-Fa-f])|007[Ff]|00[Aa][Dd]|061[Cc]|180[Ee]|200[B-Fb-f]|202[A-Ea-e]|206[0-9A-Fa-f]|[Ff][Ee][Ff][Ff]))+)(["']?[ \t]*(?:#[^\r\n]*)?(?=\r\n|\r(?!\n)|\n|$)|["']?[ \t]*:[^\r\n]*)/gu

export function nameText(s) {
  if (!INVISIBLE_GATE.test(s)) return s
  return s
    .replace(INVISIBLE_NAME, (m, run, sep, after) => {
      const name = run.replace(INVISIBLE, '')
      return (name !== run || after) && NAME_STEM.test(name) ? `${name}${sep}` : m
    })
    .replace(INVISIBLE_EXPLICIT, (m, nl, head, run, tail) => {
      const name = run.replace(INVISIBLE, '')
      return name !== run && NAME_STEM.test(name) ? `${nl}${head}${name}${tail}` : m
    })
}
// Standalone sk- credentials, with no key,
// assignment or scheme around them. The family: sk-<body>, sk-proj-, sk-svcacct-, sk-admin-,
// sk-ant-api03-, sk-ant-admin01-, sk-ant-oat01- (each sub-prefix is in the body alphabet, so one rule
// covers them all). A START is a lower-case "sk-" not preceded by a letter, digit, "_" or "-" ("risk-",
// "desk-", "task-sk-" are not starts), or preceded by a serialized JSON escape \n \r \t. A KEY is a
// start then 20+ characters of [A-Za-z0-9_-] mixing upper case, lower case AND digits; the whole run
// is replaced. Kept (not credentials to this rule): runs without all three classes ("sk-learn",
// "sk-learn-pipeline-v2"), runs under 20, other families (sk_live_, rk_, ...). There is no length or
// entropy rule anywhere else, so hashes, ids and prose keep. STREAMING: a text that ENDS inside a run
// after a start (1+ body characters) is hidden whole, since the next delta may complete a key. The
// cost: a benign run at the very end of a text ("...uses sk-learn") is hidden until text follows it,
// and in a static string for good.
// The replacement for the later-line rules in SECRET_TEXT (#396): the value on a later line hidden,
// or a block indicator's lines hidden under it, or, for a sibling or the end of the text, an anchor
// or tag left with no value on the key line hidden. Everything else is kept as written.
// A text that is a growing buffer (a reply streamed into the window or Mind) ends where the next
// delta will go on: its last line may still be arriving. There a bare value on a later line is not
// yet a scalar or a child key (it may yet become `password:`), so it is neither hidden nor
// remembered until its line ends; until then it is withheld from the served text, the one place it
// is not shown (#406). Everywhere else (a whole payload) the end of the text ends its last line.
let OPEN_END = false
function laterLine(m, nl, indent, before, name, sep, props, tail, run, br, indicator, body, prefix, q, close, offset, string) {
  const head = `${nl}${indent}${before}${name}${sep}`
  if (indicator !== undefined) return `${head}${props}${tail}${run}${br}${indicator}${/\S/.test(body) ? `${/^\r?\n|^\r/.exec(body)[0]}${/[\r\n]([ \t]*)\S/.exec(body)[1]}${REDACTED}` : body}`
  if (prefix !== undefined) {
    if (OPEN_END && q === undefined && !/[\r\n]/.test(string.slice(offset + m.length))) return `${head}${props}${tail}${run}${br}${prefix}`
    return `${head}${props}${tail}${run}${br}${prefix}${q ? `${q}${REDACTED}${close}` : REDACTED}`
  }
  return `${head}${props.replace(/^([ \t]+)[\s\S]+$/, `$1${REDACTED}`)}${tail}${run}`
}
// Each rule is [pattern, replacement, gate]: a rule with a gate runs only on a text the gate finds
// something in, a literal its pattern cannot match without, so a text skips the rules it cannot meet.
const SECRET_TEXT = [
  // Whole-value priority: a credential key's value is secret as ONE unit, whatever it
  // starts with ("Bearer ", "Basic ", even an already-redacted marker, which is untrusted input),
  // so the key rules run first and no inner pattern can split a value they recognise.
  // A quoted value runs to its UNESCAPED closing quote (JSON: `\"` and `\\` do not end it; a
  // trailing lone `\` is an escape still to come), or to the end while it is still open (a
  // streamed `KEY="…` is hidden until its quote arrives). The `=` is found first and the name
  // looked for behind it, so a long word of names is one pass, with or without a value after it.
  [/\b([A-Z0-9_]+)=(?<=(?:TOKEN|SECRET|PASSWORD|API_KEY|APIKEY)[A-Z0-9_]*=)("(?:[^"\\]|\\[\s\S]|\\$)*(?:"|$)|'(?:[^'\\]|\\[\s\S]|\\$)*(?:'|$)|`(?:[^`\\]|\\[\s\S]|\\$)*(?:`|$)|\S+)/g, (_, name) => `${name}=${REDACTED}`, /=/],
  // A block indicator with an anchor or tag before it (`password: &a |`, `!!str >-`,
  // `!<tag:yaml.org,2002:str> |`, one or two of them, each followed by a space or tab) hides
  // that property with the block: it serves as `password: |` and its block, the shape of the
  // plain form. The plain indicator, and every key line the scan below reads more broadly
  // (quoted keys, invisible characters, explicit keys past blanks), is left to `[redactYamlBlocks]`.
  // YAML allows at most one anchor and one tag on a node, so a line with three or more is
  // invalid and no rule here or below matches it: it is left as written and nothing on it is
  // remembered (#396).
  [/(^|\n)(?=[^\n]*:[ \t]*(?:[&!][^\s"'`]*[ \t]+){1,2}[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*\r?(?:\n|$))(?=([ \t]*))\2([^\n]*?(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization))([ \t]*:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2}([>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*\r?)(?=\n|$)((?:\n(?:\2[ \t]+[^\n]*|[ \t]*\r?(?=\n|$)))*)/gi, (_, nl, indent, before, name, sep, indicator, block) => `${nl}${indent}${before}${name}${sep}${indicator}${/\S/.test(block) ? `\n${indent}  ${REDACTED}${block.endsWith('\r') ? '\r' : ''}` : block}`, /:[ \t]*[&!>|]/],
  [/(^|\n)(?=[^\n]*:[ \t]*(?:[&!][^\s"'`]*[ \t]+){1,2}[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*\r?(?:\n|$))(?=([ \t]*))\2([^\n]*?(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key))([ \t]*:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2}([>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*\r?)(?=\n|$)((?:\n(?:\2[ \t]+[^\n]*|[ \t]*\r?(?=\n|$)))*)/g, (_, nl, indent, before, name, sep, indicator, block) => `${nl}${indent}${before}${name}${sep}${indicator}${/\S/.test(block) ? `\n${indent}  ${REDACTED}${block.endsWith('\r') ? '\r' : ''}` : block}`, /:[ \t]*[&!>|]/],

  // A YAML block scalar under a name (`password: |` or `>`, then the lines under it) is hidden
  // whole, its key line kept, text before the name on that line too: YAML_KEY finds its key line
  // and yamlBlockEnd where the block ends. It runs before the name rules, so they leave its `|`
  // or `>` alone.
  [redactYamlBlocks, null, /:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[&!>|]/],
  [redactExplicitKeyValues, null, /(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t]/],
  // A credential key line that ends at its colon, or at up to two anchors and tags (`&a`, `!!str`),
  // a comment after them or not, continues on a later line (#396, #410). Blank lines and comment lines
  // are skipped, and so are lines of anchors and tags alone indented under the key. Then the first
  // line strictly more indented than the key line is its value, hidden with the plain lines that
  // continue it (a quoted one up to its closing quote), or a block indicator (`|`, `>-`), whose
  // lines are hidden. A more indented mapping line (a name and a colon, quoted or not) is a child
  // key, never the value: it is left to the name rules, which hide only a credential child's value.
  // A line at the key's indent or less is a sibling: nothing is hidden or remembered from it, and an
  // anchor or tag with no value on the key line is hidden, as at the end of the text. A comment is
  // never a value, a comment line that names a credential starts nothing, and the key line and each
  // blank, comment, anchor and tag line stay as written. A line ends at \n, \r\n or a lone \r. Each
  // line is read forward from its start, its indent taken whole (a lookahead and its backreference),
  // so a key line costs one pass over it and the lines under it. An explicit key (#406), a `? name`
  // line (a list item's `- ? name` too, a trailing comment after the name, a quoted name taken
  // whole with any colon in it) then, past any blank or comment lines, a `:` line at its indent
  // with nothing after the colon but blanks, anchors, tags or a comment, is a key line too: its
  // `:` line ends it, and the lines under it are read from there. A `? name` line followed by a
  // `:` line with a block indicator (`: |`, `: >-`, anchors and tags too) is the previous rules'
  // block form.
  [/(^|(?:\r?\n|\r(?!\n)))(?=[^\r\n]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)|[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))[ \t]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?=([ \t]*))\2([^\r\n#]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization))(["']?[ \t]*:|["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))\2[ \t]*:)((?:[ \t]+[&!][^\s"'`]*){0,2})((?:[ \t]+#[^\r\n]*)?[ \t]*)((?:(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?|\2[ \t]+[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)?(?:[ \t]+#[^\r\n]*)?[ \t]*)(?=[\r\n]|$))*)(?:((?:\r?\n|\r(?!\n)))(?:(\2[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*)(?=[\r\n]|$)((?:(?:\r?\n|\r(?!\n))(?:\2[ \t]+[^\r\n]*|[ \t]*(?=[\r\n]|$)))*)|(\2[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}(?![ \t&!#\r\n]|[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?=[\r\n]|$)|(?:"[^"\r\n]*"|'[^'\r\n]*'|(?:[^\r\n: \t]|[ \t]+(?=[^\r\n: \t#]))+)[ \t]*:(?:[ \t]|(?=[\r\n]|$)))(?:(?:Bearer|Basic)[ \t]+(?=[A-Za-z0-9._~+/=-]))?)(?:(["'`])(?:(?!\13)[^\\]|\\[\s\S]|\\$)*(\13|$)|(?:[^ \t\r\n]|[ \t]+(?![ \t#\r\n]|$))+(?:(?:(?:\r?\n|\r(?!\n))[ \t]*(?=[\r\n]))*(?:\r?\n|\r(?!\n))\2[ \t]+(?![ \t#\r\n])[^\r\n]*)*))|(?=(?:\r?\n|\r(?!\n))(?!\2[ \t])[ \t]*[^\s#]|$))/gi, laterLine, /:[ \t]*(?:[&!#][^\r\n]*)?(?:[\r\n]|$)/],
  [/(^|(?:\r?\n|\r(?!\n)))(?=[^\r\n]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)|[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))[ \t]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?=([ \t]*))\2([^\r\n#]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key))(["']?[ \t]*:|["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))\2[ \t]*:)((?:[ \t]+[&!][^\s"'`]*){0,2})((?:[ \t]+#[^\r\n]*)?[ \t]*)((?:(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?|\2[ \t]+[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)?(?:[ \t]+#[^\r\n]*)?[ \t]*)(?=[\r\n]|$))*)(?:((?:\r?\n|\r(?!\n)))(?:(\2[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*)(?=[\r\n]|$)((?:(?:\r?\n|\r(?!\n))(?:\2[ \t]+[^\r\n]*|[ \t]*(?=[\r\n]|$)))*)|(\2[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}(?![ \t&!#\r\n]|[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?=[\r\n]|$)|(?:"[^"\r\n]*"|'[^'\r\n]*'|(?:[^\r\n: \t]|[ \t]+(?=[^\r\n: \t#]))+)[ \t]*:(?:[ \t]|(?=[\r\n]|$)))(?:(?:Bearer|bearer|BEARER|Basic|basic|BASIC)[ \t]+(?=[A-Za-z0-9._~+/=-]))?)(?:(["'`])(?:(?!\13)[^\\]|\\[\s\S]|\\$)*(\13|$)|(?:[^ \t\r\n]|[ \t]+(?![ \t#\r\n]|$))+(?:(?:(?:\r?\n|\r(?!\n))[ \t]*(?=[\r\n]))*(?:\r?\n|\r(?!\n))\2[ \t]+(?![ \t#\r\n])[^\r\n]*)*))|(?=(?:\r?\n|\r(?!\n))(?!\2[ \t])[ \t]*[^\s#]|$))/g, laterLine, /:[ \t]*(?:[&!#][^\r\n]*)?(?:[\r\n]|$)/],
  // `name: value` / `name=value`, in JSON too. A quoted value ("…", '…' or `…`; spaces and escapes
  // included) runs to its unescaped closing quote, or to the end while open. After a name, an
  // authorization scheme in any case (`authorization: bearer x`) keeps its word and hides what
  // follows; a scheme with no name is the case-sensitive rule below, so "basic steps" stays prose.
  // A name is passphrase, password, passwd, pass, secret, secret key, secret access key, token, api
  // key, private key or authorization, in any case, as a whole token with any prefix joined by _ or
  // - (db_password, x-api-key, aws_secret_access_key), also right after a - or -- (--api-key=x):
  // `bypass` and `tokenizer` are words, not names.
  // After a name and its colon, up to two YAML anchors and tags (`&a`, `!!str`, `!secret`, in either
  // order, each followed by a space or tab, on the colon's line) come before the value: they are
  // hidden with it, so `password: &a x` serves as `password: [redacted]`, the shape of the plain
  // scalar. The skip never crosses a line break, so `password: &a` then `username: bob` hides only
  // `&a` and the next line's key is never read, or remembered, as the value. Anchors and tags
  // then a block indicator (and a comment) at the end of the line are a block scalar's key line
  // (the rules above), never a value, so neither an anchor nor a tag is read as one. YAML allows
  // at most one anchor and one tag on a node: a line with three or more before its value or its
  // indicator is invalid YAML, and these rules do not match it, so it is left as written, no anchor
  // or tag is hidden as its value and nothing on it is remembered; the block rules do not read it
  // either.
  // A key line that ends at its colon, or at its anchors and tags, or at a comment, is the later-line
  // rule's above, never these rules': here a name with no value on its line reads no later line, and
  // a comment is not a value. A quoted name with nothing after its colon (`"password":`) still reads
  // the next line, as JSON may break there. A quoted name is taken whole, a colon inside it
  // included. An explicit key's `? name` line whose value sits on its `:` line, past a trailing
  // comment or a gap, is the gap rules below: they run after these, so a comment's own text is read
  // by these rules first (#500).
  // An explicit key's name is one token to these rules and to the gap rules below: the first token
  // of a `? name` line (a list item's too, anchors and tags allowed before it) is taken whole,
  // quoted or not, so a colon in the name is never read as a name and a value and no part of the
  // name is remembered (#509). A quoted name is recognised while its quote closes on its line; an
  // unquoted one is the token right after `? `, and its colon has no blank after it (with one it is
  // the name's value on its own line). The guard sits at the name/separator boundary: it reads the
  // line's end, any gap, and the `:` line, so a `? name` line with no `:` line of its own is left
  // to these rules as before. Its bound is 240 characters of name and tail; past it, or with a
  // quote left open on its line, the line is read as text here and the value on the `:` line is
  // hidden by the gap rules' opaque-name branch -- over-redaction (a name's tail with it), never
  // service. A trailing comment is still read by these rules (#500).
  [/(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))((?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization))(?!(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,40}"[^"\r\n]{0,240})(?=[^"\r\n]{0,240}"|(?![^"\r\n]*:[ \t])[^"\r\n]*[\r\n]))(?!(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,40}'[^'\r\n]{0,240})(?=[^'\r\n]{0,240}'|(?![^'\r\n]*:[ \t])[^'\r\n]*[\r\n]))(?!(?=:(?![ \t\r\n])(?:(?!:[ \t])[^ \t"'#\r\n]){0,240}[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?(?:\r?\n|\r(?!\n)))*[ \t]*:)(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[A-Za-z0-9_-]{1,240}))((?:["']?\s*=|\s*:(?!(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))|["']\s*:(?!(?=[ \t]+[&!#])(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)))\s*)(?!(?:[&!][^\s"'`]*[ \t]+){3})(?!(?:[&!][^\s"'`]*[ \t]+)*[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(?:(["'`])(?:(?!\3)[^\\]|\\[\s\S]|\\$)*(\3|$)|(Bearer|Basic)\s+[A-Za-z0-9._~+/=-]+|(?![>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?!Bearer\b|Basic\b|\[redacted\])[^\s"',}]+)/gi, (_, name, sep, q, close, scheme) => (q ? `${name}${sep}${q}${REDACTED}${close}` : scheme ? `${name}${sep}${scheme} ${REDACTED}` : `${name}${sep}${REDACTED}`), /[:=]/],
  // The same names in camelCase or PascalCase (accessToken, ClientSecret, dbPassword, DbPass,
  // apiKey, authToken), case sensitive and whole-token, so accessLevel, MaxTokens, passRate and
  // PassengerCount stay words.
  // A name is a key only with its value, so a firstPass in prose stays a word.
  [/(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))((?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key))(?!(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,40}"[^"\r\n]{0,240})(?=[^"\r\n]{0,240}"|(?![^"\r\n]*:[ \t])[^"\r\n]*[\r\n]))(?!(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,40}'[^'\r\n]{0,240})(?=[^'\r\n]{0,240}'|(?![^'\r\n]*:[ \t])[^'\r\n]*[\r\n]))(?!(?=:(?![ \t\r\n])(?:(?!:[ \t])[^ \t"'#\r\n]){0,240}[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?(?:\r?\n|\r(?!\n)))*[ \t]*:)(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[A-Za-z0-9_-]{1,240}))((?:["']?\s*=|\s*:(?!(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))|["']\s*:(?!(?=[ \t]+[&!#])(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)))\s*)(?!(?:[&!][^\s"'`]*[ \t]+){3})(?!(?:[&!][^\s"'`]*[ \t]+)*[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(?:(["'`])(?:(?!\3)[^\\]|\\[\s\S]|\\$)*(\3|$)|(Bearer|bearer|BEARER|Basic|basic|BASIC)\s+[A-Za-z0-9._~+/=-]+|(?![>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?!(?:Bearer|bearer|BEARER|Basic|basic|BASIC)\b|\[redacted\])[^\s"',}]+)/g, (_, name, sep, q, close, scheme) => (q ? `${name}${sep}${q}${REDACTED}${close}` : scheme ? `${name}${sep}${scheme} ${REDACTED}` : `${name}${sep}${REDACTED}`), /Token|Secret|Pass|Key|Authorization/],
  // The explicit key's gap form (#496, #500): a `? name` line whose value sits on its `:` line,
  // past a trailing comment and any blank or comment lines. It runs AFTER the two rules above,
  // so the comment's own text is read by them first: a match spans the comment, the replacement
  // writes it back, and the scan continues after the value, so text a match swallows is never
  // read again (#500 served a credential in the comment that way). Both the trailing comment and
  // each gap line end at their own line end, so each matches whole and cannot back up into a
  // colon inside it (which once made a comment line the value, #500), and the `:` that ends the
  // gap must be the first non-blank character of its own line: a colon mid-line is not a
  // separator, and a `? name` line with nothing after it is not a key line here. The name may be
  // unquoted with colons in it (`? password:primary`: one token, a colon with no blank after it),
  // and a quoted one whose quote closes past the bound or not at all is opaque: both are taken
  // whole, so the value under the `:` line is hidden and remembered and no part of the name is (a
  // bound is 240 characters; an opaque name is 241 or more, or unclosed on its line).
  [/(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))((?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])))((?:["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?(?:\r?\n|\r(?!\n)))*[ \t]*:(?!(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)))[ \t]*)(?!(?:[&!][^\s"'`]*[ \t]+){3})(?!(?:[&!][^\s"'`]*[ \t]+)*[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(?:(["'`])(?:(?!\3)[^\\]|\\[\s\S]|\\$)*(\3|$)|(Bearer|Basic)\s+[A-Za-z0-9._~+/=-]+|(?![>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?!Bearer\b|Basic\b|\[redacted\])[^\s"',}]+)/gi, (_, name, sep, q, close, scheme) => (q ? `${name}${sep}${q}${REDACTED}${close}` : scheme ? `${name}${sep}${scheme} ${REDACTED}` : `${name}${sep}${REDACTED}`), /[:=]/],
  [/(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))((?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])))((?:["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?(?:\r?\n|\r(?!\n)))*[ \t]*:(?!(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)))[ \t]*)(?!(?:[&!][^\s"'`]*[ \t]+){3})(?!(?:[&!][^\s"'`]*[ \t]+)*[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(?:(["'`])(?:(?!\3)[^\\]|\\[\s\S]|\\$)*(\3|$)|(Bearer|bearer|BEARER|Basic|basic|BASIC)\s+[A-Za-z0-9._~+/=-]+|(?![>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?!(?:Bearer|bearer|BEARER|Basic|basic|BASIC)\b|\[redacted\])[^\s"',}]+)/g, (_, name, sep, q, close, scheme) => (q ? `${name}${sep}${q}${REDACTED}${close}` : scheme ? `${name}${sep}${scheme} ${REDACTED}` : `${name}${sep}${REDACTED}`), /Token|Secret|Pass|Key|Authorization/],
  // An unclosed quoted explicit key with a no-space colon on its line (`? "password:hunter2`):
  // the tail reads as the value; mask it to the line end here, text only. The clean-close skip
  // uses the opening quote only, so a mismatched quote cannot pass. A quote of the other kind
  // inside the name is part of it (`? "user's_password:hunter2`, #530): the scan before the
  // credential and the `:` may cross it, or the line would serve its value.
  [/(^|[\r\n])((?:[ \t]*(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]+){0,2})(["'])(?=(?:(?!\3)[^\r\n:])*?(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization))(?:(?!\3)[^\r\n:])*:(?![ \t])(?![^"'\r\n]*(?<!\\)\3[ \t]*(?::[^\r\n]*|#[^\r\n]*)?(?=[\r\n]|$)))([^\r\n]+)(?=[\r\n]|$)/g, (_, nl, prefix) => `${nl}${prefix}${REDACTED}`, /:/],
  [/(^|[\r\n])((?:[ \t]*(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]+){0,2})(["'])(?=(?:(?!\3)[^\r\n:])*?(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key))(?:(?!\3)[^\r\n:])*:(?![ \t])(?![^"'\r\n]*(?<!\\)\3[ \t]*(?::[^\r\n]*|#[^\r\n]*)?(?=[\r\n]|$)))([^\r\n]+)(?=[\r\n]|$)/g, (_, nl, prefix) => `${nl}${prefix}${REDACTED}`, /:/],
  // JSON inside a JSON string (`{\"password\": \"…\"}`): the value runs to its escaped closing
  // quote, its own escapes doubled, or to a bare quote that ends the outer string.
  [/(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))((?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization))(\\"\s*:\s*)\\"((?:[^\\"]|\\[^\\"]|\\\\(?:\\[\s\S]|\\$|[^\\]|$)|\\$)*)(\\"|"|$)/gi, (_, name, sep, value, close) => `${name}${sep}\\"${REDACTED}${close}`, /\\"/],
  [/(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))((?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key))(\\"\s*:\s*)\\"((?:[^\\"]|\\[^\\"]|\\\\(?:\\[\s\S]|\\$|[^\\]|$)|\\$)*)(\\"|"|$)/g, (_, name, sep, value, close) => `${name}${sep}\\"${REDACTED}${close}`, /\\"/],
  [/\b(Bearer|Basic)\s+[A-Za-z0-9._~+/=-]+/g, (_, scheme) => `${scheme} ${REDACTED}`],
  // A scheme in another case with no name before it ("bearer x9…") hides only a token-shaped word:
  // 16+ characters of the token alphabet with a digit, so "bearer bonds" and "basic steps" stay
  // prose. A run still open at the end of the text is hidden until it closes, so a token streamed
  // in parts shows no prefix.
  [/\b(bearer|basic)\s+(?=[A-Za-z0-9._~+/=-]*[0-9])[A-Za-z0-9._~+/=-]{16,}/gi, (_, scheme) => `${scheme} ${REDACTED}`],
  [/\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]+$/i, (_, scheme) => `${scheme} ${REDACTED}`],
  [/\bAIza[0-9A-Za-z_-]{20,}/g, () => REDACTED],
  // Standalone sk- credentials (the contract above): a whole key, then a still-open run at the end.
  [/(?:(?<![A-Za-z0-9_-])|(?<=\\[nrt]))sk-(?=[A-Za-z0-9_-]*[A-Z])(?=[A-Za-z0-9_-]*[a-z])(?=[A-Za-z0-9_-]*[0-9])[A-Za-z0-9_-]{20,}/g, () => REDACTED],
  [/(?:(?<![A-Za-z0-9_-])|(?<=\\[nrt]))sk-[A-Za-z0-9_-]+$/, () => REDACTED],
  // A line a cut padded with blanks (#406 F-B): a run of blanks as long as the keep is the cut's
  // padding, not text. Its head is gone — the name on it cannot be read (a child key arrives as
  // "pass" and "word:") — so once the line has ended it is hidden whole, and while it is still
  // arriving (a growing buffer's last line, OPEN_END) its rest is withheld, as the value that
  // completes it is not remembered yet (#406's last frame). It runs LAST, on purpose: a padded line
  // that is a key line still names the value on the lines under it, and the rules above hide that
  // value first; hiding this line first would destroy the name and leave the value orphaned and
  // served, however the deltas were split (#406 CLASS). A pad-only line is the blank line the cut
  // padded: it holds nothing to hide, and the rules above read its blanks as the blank line it is
  // (a key line skips it to its value).
  [/^[^\n]*$/gm, (line, at, all) => {
    // A pad is read from its run's first blank only, so a run just short of the keep is scanned once,
    // not again from each of its blanks (it was quadratic in a growing buffer's padded last line).
    const pad = /(?<![ \t])[ \t]{4096,}/.exec(line)
    if (!pad) return line
    if (!/[^ \t]/.test(line)) return ' '
    // While the line is still arriving (a growing buffer's last line) everything after the pad is
    // withheld: the head is cut, so the line cannot be read, and no rule above has served it.
    return OPEN_END && at + line.length === all.length ? line.slice(0, pad.index + pad[0].length) : REDACTED
  }, /(?<![ \t])[ \t]{4096,}/],
]

// A YAML key line under a credential name, read as untrusted text, not validated YAML: from the
// line end before it (group 1) and its indent (2), the name anywhere on the line as a whole token,
// quoted or not, then its `:`, an anchor (&a) and a tag (!!str) in either order, the `|` or `>`
// with its indicators and a comment, up to its own line end. A character that prints nothing
// (NUL, DEL, zero-width, BOM) counts as a space between them. A quoted name is read whole, a colon
// inside it included. The last two read an explicit key (`? password`, its `: |` on a later line,
// past any blank or comment lines): their indent is the key line's when the `:` line starts with
// it, otherwise none, so the block is hidden whole either way. Each is one pass: a blank or comment
// line skips no gap as a key line, so a gap is read once.
const YAML_KEY = [
  /(^|\r\n|\r(?!\n)|\n)(?=[^\r\n]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[&!>|])(?=([ \t]*))\2([^\r\n]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)))((?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*["'])?[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:[&!][^&!\r\n\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*){0,3}[>|][-+0-9]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?=[\r\n]|$))/gi,
  /(^|\r\n|\r(?!\n)|\n)(?=[^\r\n]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[&!>|])(?=([ \t]*))\2([^\r\n]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)))((?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*["'])?[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:[&!][^&!\r\n\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*){0,3}[>|][-+0-9]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?=[\r\n]|$))/g,
  /(^|\r\n|\r(?!\n)|\n)(?=(?:(?![\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#|[\r\n]|$))[^\r\n]*(?:\r\n|\r(?!\n)|\n)(?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r(?!\n)|\n))*|[^\r\n]*(?:\r\n|\r|\n))[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:[&!][^&!\r\n\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*){0,3}[>|][-+0-9]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?=[\r\n]|$))(?=((?:(?=([ \t]*))\3(?=[^\r\n]*(?:\r\n|\r|\n)(?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r(?!\n)|\n))*\3[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:))?))([^\r\n]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n]))))((?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*["'])?[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r|\n)(?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r(?!\n)|\n))*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:[&!][^&!\r\n\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*){0,3}[>|][-+0-9]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?=[\r\n]|$))/gi,
  /(^|\r\n|\r(?!\n)|\n)(?=(?:(?![\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#|[\r\n]|$))[^\r\n]*(?:\r\n|\r(?!\n)|\n)(?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r(?!\n)|\n))*|[^\r\n]*(?:\r\n|\r|\n))[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:[&!][^&!\r\n\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*){0,3}[>|][-+0-9]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?=[\r\n]|$))(?=((?:(?=([ \t]*))\3(?=[^\r\n]*(?:\r\n|\r|\n)(?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r(?!\n)|\n))*\3[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:))?))([^\r\n]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt])))((?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n]))))((?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*["'])?[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r|\n)(?:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?:\r\n|\r(?!\n)|\n))*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*:[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:[&!][^&!\r\n\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*){0,3}[>|][-+0-9]*[\x00-\x09\x0B\x0C\x0E-\x20\x7F-\xA0\xAD\u061C\u1680\u180E\u2000-\u200F\u2028-\u202F\u205F-\u2064\u3000\uFEFF]*(?:#[^\r\n]*)?(?=[\r\n]|$))/g,
]

/**
 * Where the block under a YAML key line ends: `at` is the key line's own line end, `indent` its
 * indent. A line scanner. The key line's line end (\n, \r\n or a lone \r, each one line end; an
 * LF or CR key's \r\n too) ends a block line, and any other \r or \n is a stray inside the value.
 * A line ends the block only when its first other character comes after spaces alone, no more of
 * them than its key's indent (a prefix of it). Any other line is value: a blank one, a deeper one,
 * one with a tab, a stray or a character that prints nothing (NUL, DEL, zero-width) before its
 * first other character. The block ends before the line end of that line, the less-indented line
 * after it, shown.
 */
function yamlBlockEnd(s, at, indent) {
  const eol = s.startsWith('\r\n', at) ? '\r\n' : s.charAt(at)
  let end = at
  while (eol && s.startsWith(eol, end)) {
    let from = end + eol.length
    if (eol === '\r' && s[from] === '\n') from++
    let next = s.indexOf(eol, from)
    if (next < 0) next = s.length
    const lead = /^[\s\x00-\x08\x0E-\x1F\x7F-\x9F\xAD\u061C\u180E\u200B-\u200F\u202A-\u202E\u2060-\u2064]*/.exec(s.slice(from, next))[0]
    if (from + lead.length < next && /^ *$/.test(lead) && indent.startsWith(lead)) break
    end = next
  }
  return end
}

// The YAML rule of SECRET_TEXT: a block with more than whitespace becomes one line under its key,
// the key line's own line end and indent then `  [redacted]`.
function redactYamlBlocks(s) {
  let out = s
  for (const re of YAML_KEY) {
    let done = ''
    let from = 0
    re.lastIndex = 0
    for (let m = re.exec(out); m; m = re.exec(out)) {
      const at = m.index + m[0].length
      const end = yamlBlockEnd(out, at, m[2])
      const block = out.slice(at, end)
      done += `${out.slice(from, at)}${/\S/.test(block) ? `${block.match(/^(?:\r\n?|\n)/)[0]}${m[2]}  ${REDACTED}` : block}`
      from = re.lastIndex = end
    }
    out = `${done}${out.slice(from)}`
  }
  return out
}
// #507: a value on the indented lines under an explicit key. A `? name` line whose name holds a
// credential stem (quoted or not, a list item, anchors and tags before it, a colon inside the
// name) claims the more-indented lines under it as a value: a scalar line and a child key's value
// are hidden there, and once such a line sits in the gap its `:` line's value is hidden too (an
// inline value, the lines under a block indicator, or the lines under an empty colon). A clean
// `? name` ... `:` shape is left to the gap and block rules. Read as untrusted text, not as
// validated YAML: over-hiding a value block is the cost, never service.
const KEY_NAME_LOWER = /(?:(?<![A-Za-z0-9_-])|(?<=[_-]))(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-])/i
const KEY_NAME_CAMEL = /(?:(?<![A-Za-z0-9_-])|(?<=[_-]))(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-])/
const KEY_PROPS = /^(?:[&!][^\s"'`]*[ \t]+){0,2}/
const KEY_BLOCK = /^[>|][-+0-9]*[ \t]*(?:#[^\r\n]*)?$/

/**
 * The name on an explicit key line: a quoted token whole (any colon in it), or the first token. A
 * quoted name ends only at its real closing quote: an escaped \" inside double quotes and a doubled
 * '' inside single quotes are part of the name. With no closing quote, the rest of the line is it.
 */
function explicitKeyName(text) {
  const tail = text.slice(KEY_PROPS.exec(text)[0].length)
  const quote = tail[0]
  if (quote === '"' || quote === "'") {
    for (let i = 1; i < tail.length; i++) {
      if (quote === '"' && tail[i] === '\\') i++
      else if (tail[i] === quote && quote === "'" && tail[i + 1] === "'") i++
      else if (tail[i] === quote) return tail.slice(1, i)
    }
    return tail.slice(1)
  }
  return /^\S*/.exec(tail)[0]
}

function credentialName(name) {
  const plain = INVISIBLE_GATE.test(name) ? name.replace(INVISIBLE, '') : name
  return KEY_NAME_LOWER.test(plain) || KEY_NAME_CAMEL.test(plain)
}

/**
 * The value a hidden line holds, for `secretsIn`: a child key's value, else the line's first word.
 * A list item's `- ` marks are not the value: the item after them is.
 */
function explicitLineValue(content) {
  const item = content.replace(/^(?:-[ \t]+)+/, '')
  const child = /^[^ \t"'][^:]*?:[ \t]+(\S.*)$/.exec(item)
  const text = child ? child[1] : item
  const quoted = /^(["'`])((?:\\.|(?!\1)[^\\])*?)\1/.exec(text)
  if (quoted) return quoted[2]
  const first = /^(?:[&!][^\s"'`]*[ \t]+)*([^\s"'`]+)/.exec(text)
  return first ? first[1] : ''
}

/** The `:` line's value: an inline one hidden in place, or the lines under it hidden whole. */
function explicitColonHits(s, line, start, lineEnd) {
  const at = line.indexOf(':')
  const blanks = /^[ \t]*/.exec(line.slice(at + 1))[0]
  const props = KEY_PROPS.exec(line.slice(at + 1 + blanks.length))[0]
  const rest = line.slice(at + 1 + blanks.length + props.length)
  const hits = []
  if (rest && rest[0] !== '#' && !KEY_BLOCK.test(rest)) {
    hits.push({ start, end: lineEnd, text: `${line.slice(0, at + 1)}${blanks}${REDACTED}`, value: explicitLineValue(rest), ended: lineEnd })
    return hits
  }
  // A block indicator or an empty colon: the lines under it, more indented than it, are its value.
  let cursor = lineEnd
  if (s.startsWith('\r\n', cursor)) cursor += 2
  else if (s[cursor] === '\r' || s[cursor] === '\n') cursor += 1
  else return hits
  const indent = /^[ \t]*/.exec(line)[0].length
  let first = true
  while (cursor < s.length) {
    let end = cursor
    while (end < s.length && s[end] !== '\r' && s[end] !== '\n') end++
    const body = s.slice(cursor, end)
    const lead = /^[ \t]*/.exec(body)[0]
    const content = body.slice(lead.length)
    if (!content || content[0] === '#') { /* a blank or comment line stays */ }
    else if (lead.length > indent) {
      hits.push({ start: cursor, end, text: `${lead}${REDACTED}`, value: first ? explicitLineValue(content) : '', ended: end })
      first = false
    } else break
    if (s.startsWith('\r\n', end)) cursor = end + 2
    else cursor = end + 1
  }
  return hits
}

/**
 * The lines under an explicit credential key that hide a value (#507), as replacements. The `?`
 * line itself stays as written. A key line already covered by an outer key's block is skipped, so
 * nested keys cost one pass.
 */
function explicitKeyHits(s) {
  if (!s.includes('?')) return []
  const hits = []
  const keyLine = /(^|[\r\n])([ \t]*)(?:-[ \t]+)?\?[ \t]([^\r\n]*)/g
  let covered = 0
  for (let m = keyLine.exec(s); m; m = keyLine.exec(s)) {
    if (m.index < covered) continue
    if (!credentialName(explicitKeyName(m[3]))) {
      // A plain name of several words (`? db password`): the rest of the line, minus a comment.
      const rest = m[3].slice(KEY_PROPS.exec(m[3])[0].length).replace(/(?:^|[ \t])#.*$/, '')
      if (!credentialName(rest)) continue
    }
    // #530: a plain name of several words (`? password db`) is more than the one token the gap
    // rules read, so its `:` line right under it is taken here even with nothing in the gap.
    const plain = m[3].slice(KEY_PROPS.exec(m[3])[0].length).replace(/(?:^|[ \t])#.*$/, '')
    const several = !/^["']/.test(plain) && /\S[ \t]+\S/.test(plain)
    const keyIndent = m[2].length
    let cursor = m.index + m[0].length
    if (s.startsWith('\r\n', cursor)) cursor += 2
    else if (s[cursor] === '\r' || s[cursor] === '\n') cursor += 1
    else continue
    let seen = false
    while (cursor < s.length) {
      let end = cursor
      while (end < s.length && s[end] !== '\r' && s[end] !== '\n') end++
      const line = s.slice(cursor, end)
      const lead = /^[ \t]*/.exec(line)[0]
      const content = line.slice(lead.length)
      if (content && content[0] === ':' && lead.length >= keyIndent) {
        if (seen || several) hits.push(...explicitColonHits(s, line, cursor, end))
        break
      }
      if (content && content[0] !== '#' && lead.length > keyIndent) {
        hits.push({ start: cursor, end, text: `${lead}${REDACTED}`, value: explicitLineValue(content), ended: end })
        seen = true
      } else if (content && content[0] !== '#' && lead.length <= keyIndent) break
      if (s.startsWith('\r\n', end)) cursor = end + 2
      else cursor = end + 1
    }
    if (hits.length) covered = Math.max(covered, hits[hits.length - 1].end)
  }
  return hits
}

function redactExplicitKeyValues(s) {
  const hits = explicitKeyHits(s)
  if (!hits.length) return s
  hits.sort((a, b) => a.start - b.start)
  let out = ''
  let from = 0
  for (const hit of hits) {
    if (hit.start < from) continue
    out += `${s.slice(from, hit.start)}${hit.text}`
    from = hit.end
  }
  return out + s.slice(from)
}

// A closed quoted explicit name followed by more text is malformed YAML. Read its whole
// line as an opaque name and hide any indented value and following colon value (#785).
function redactMalformedExplicitKeys(s) {
  if (!/\?[ \t]/.test(s)) return s
  const keys = /(^|\r\n|\r(?!\n)|\n)([ \t]*)(-[ \t]+)?\?[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}("(?:[^"\\\r\n]|\\[^\r\n])*"|'(?:[^'\r\n]|'')*')([^\r\n]*)/g
  let done = ''
  let from = 0
  for (let m = keys.exec(s); m; m = keys.exec(s)) {
    if (/^[ \t]*(?:#[^\r\n]*|:[^\r\n]*)?$/.test(m[5]) || !NAME_STEM.test(m[4] + m[5])) continue
    const indent = m[2] + (m[3] ? ' '.repeat(m[3].length) : '')
    const at = m.index + m[0].length
    let end = yamlBlockEnd(s, at, indent)
    const colon = /^(?:(?:\r\n|\r(?!\n)|\n)[ \t]*(?:#[^\r\n]*)?)*(?:\r\n|\r(?!\n)|\n)([ \t]*):[^\r\n]*/.exec(s.slice(end))
    if (colon && colon[1] === indent) {
      end += colon[0].length
      end = yamlBlockEnd(s, end, indent)
    }
    const block = s.slice(at, end)
    done += s.slice(from, at) + (/\S/.test(block) ? `${block.match(/^(?:\r\n|\r|\n)/)[0]}${indent}  ${REDACTED}` : block)
    from = keys.lastIndex = end
  }
  return done + s.slice(from)
}

export function redactText(s, open = false) {
  const was = OPEN_END
  OPEN_END = open
  try {
    let out = redactMalformedExplicitKeys(nameText(s))
    for (const [re, fn, gate] of SECRET_TEXT) if (!gate || gate.test(out)) out = typeof re === 'function' ? re(out) : out.replace(re, fn)
    return out
  } finally {
    OPEN_END = was
  }
}

export function redact(value, key = '') {
  if (key && SECRET_KEY.test(key.replace(INVISIBLE, '')) && value !== null && value !== undefined && typeof value !== 'boolean') return REDACTED
  if (typeof value === 'string') return redactText(value)
  if (Array.isArray(value)) return value.map((v) => redact(v))
  if (value && typeof value === 'object') {
    const out = {}
    for (const [k, v] of Object.entries(value)) out[k] = redact(v, k)
    return out
  }
  return value
}

// The credential VALUES the text rules above would replace, so a caller that later sees the same
// text without its context (a tail cut away "Bearer ", "key=" or "Authorization: ") can still
// mask them. The values are returned to the caller only, never stored by this module.
// A pattern, [pattern, gate], or [pattern, gate, 'ended'] as in SECRET_TEXT. A bare value under a
// name never starts with & or ! (`password: !!null`, an anchor or tag with no value after it), so a
// later literal `!!null` is not masked; the text rules still hide it in place. A value on a later
// line (#396) is remembered as the later-line rule finds it (its first word, or its quoted text; a
// block's lines are not, as on its key's line, but an explicit key's block remembers its first
// line's first value); the key line's indent is a group before it, so the value is the last group a
// match defines. It is remembered once its line has ended (#406): a blank after it is not enough, as
// a key may hold one and a cut pads with them, so a child key still arriving is never remembered.
// An explicit key's name may be quoted whole (any colon in it) and may end at a trailing comment or
// at blank or comment lines before its `:` line; its value is remembered the same way either side.
// An explicit key's continued value is remembered when its line ends at the end of the text too
// (#406 F-C), as the bases did; that entry, an explicit key's block and a padded line's value are
// 'ended': a growing buffer (secretsIn's `open`) skips a match only while its last line may still
// be arriving, one that ends at the end of the text, and remembers it once anything follows the
// value there, a line break, a blank or a comment (#406 FAIL 2, NARROW 4).
const SECRET_VALUES = [
  /\b(?:Bearer|Basic)\s+([A-Za-z0-9._~+/=-]+)/g,
  /\b(?:bearer|basic)\s+((?=[A-Za-z0-9._~+/=-]*[0-9])[A-Za-z0-9._~+/=-]{16,})(?=[^A-Za-z0-9._~+/=-])/gi, // a whole, closed token-shaped word only
  /\b(AIza[0-9A-Za-z_-]{20,})/g,
  // A padded line's value (#406 F-B): its head is gone, so its name cannot be read (a child key
  // arrives as "pass" and "word:"), and the value after the pad's first colon is remembered
  // fail-closed, once it has ended, so a later bare occurrence is masked too. The line itself is
  // hidden whole by SECRET_TEXT; a quoted value there is not remembered, only hidden.
  [/(?:^|[^ \t])[ \t]{4096,}(?=[^ \t])[^\n]*?[:=][ \t]*([^\s"',}\r\n]+)/g, /(?<![ \t])[ \t]{4096,}/, 'ended'],
  /(?:(?<![A-Za-z0-9_-])|(?<=\\[nrt]))(sk-(?=[A-Za-z0-9_-]*[A-Z])(?=[A-Za-z0-9_-]*[a-z])(?=[A-Za-z0-9_-]*[0-9])[A-Za-z0-9_-]{20,})(?=[^A-Za-z0-9_-])/g, // a whole, CLOSED sk- key only: a run still open at the end of the text is never remembered, so a key streamed in parts cannot fill SECRETS_KEPT with its prefixes
  [/\b[A-Z0-9_]+=(?<=(?:TOKEN|SECRET|PASSWORD|API_KEY|APIKEY)[A-Z0-9_]*=)("(?:[^"\\]|\\[\s\S]|\\$)*(?:"|$)|'(?:[^'\\]|\\[\s\S]|\\$)*(?:'|$)|`(?:[^`\\]|\\[\s\S]|\\$)*(?:`|$)|\S+)/g, /=/],
  [/(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization))(?!(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,40}"[^"\r\n]{0,240})(?=[^"\r\n]{0,240}"|(?![^"\r\n]*:[ \t])[^"\r\n]*[\r\n]))(?!(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,40}'[^'\r\n]{0,240})(?=[^'\r\n]{0,240}'|(?![^'\r\n]*:[ \t])[^'\r\n]*[\r\n]))(?!(?=:(?![ \t\r\n])(?:(?!:[ \t])[^ \t"'#\r\n]){0,240}[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?(?:\r?\n|\r(?!\n)))*[ \t]*:)(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[A-Za-z0-9_-]{1,240}))(?:["']?\s*=|\s*:(?!(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))|["']\s*:(?!(?=[ \t]+[&!#])(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)))\s*(?!(?:[&!][^\s"'`]*[ \t]+){3})(?!(?:[&!][^\s"'`]*[ \t]+)*[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(?:"((?:[^"\\]|\\[\s\S]|\\$)+)|'((?:[^'\\]|\\[\s\S]|\\$)+)|`((?:[^`\\]|\\[\s\S]|\\$)+)|(?:Bearer|Basic)\s+([A-Za-z0-9._~+/=-]+)|(?!Bearer\b|Basic\b|\[redacted\]|[&!])([^\s"',}]+))/gi, /[:=]/],
  [/(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key))(?!(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,40}"[^"\r\n]{0,240})(?=[^"\r\n]{0,240}"|(?![^"\r\n]*:[ \t])[^"\r\n]*[\r\n]))(?!(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,40}'[^'\r\n]{0,240})(?=[^'\r\n]{0,240}'|(?![^'\r\n]*:[ \t])[^'\r\n]*[\r\n]))(?!(?=:(?![ \t\r\n])(?:(?!:[ \t])[^ \t"'#\r\n]){0,240}[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?(?:\r?\n|\r(?!\n)))*[ \t]*:)(?<=(?:^|[\r\n])[ \t]{0,160}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[A-Za-z0-9_-]{1,240}))(?:["']?\s*=|\s*:(?!(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))|["']\s*:(?!(?=[ \t]+[&!#])(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)))\s*(?!(?:[&!][^\s"'`]*[ \t]+){3})(?!(?:[&!][^\s"'`]*[ \t]+)*[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(?:"((?:[^"\\]|\\[\s\S]|\\$)+)|'((?:[^'\\]|\\[\s\S]|\\$)+)|`((?:[^`\\]|\\[\s\S]|\\$)+)|(?:Bearer|bearer|BEARER|Basic|basic|BASIC)\s+([A-Za-z0-9._~+/=-]+)|(?!(?:Bearer|bearer|BEARER|Basic|basic|BASIC)\b|\[redacted\]|[&!])([^\s"',}]+))/g, /Token|Secret|Pass|Key|Authorization/],
  // The explicit key's gap form, as the text rules read it (#500): its `? name` line, the
  // trailing comment and gap lines, then a `:` at the start of its own line. It runs after the
  // two entries above, so a credential-shaped value in the comment is already one of their own
  // (and is remembered), and a comment's text is never the value remembered here: each comment
  // run ends at its own line end, so it matches whole.
  [/(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])))(?:["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?(?:\r?\n|\r(?!\n)))*[ \t]*:(?!(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)))[ \t]*(?!(?:[&!][^\s"'`]*[ \t]+){3})(?!(?:[&!][^\s"'`]*[ \t]+)*[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(?:"((?:[^"\\]|\\[\s\S]|\\$)+)|'((?:[^'\\]|\\[\s\S]|\\$)+)|`((?:[^`\\]|\\[\s\S]|\\$)+)|(?:Bearer|Basic)\s+([A-Za-z0-9._~+/=-]+)|(?!Bearer\b|Basic\b|\[redacted\]|[&!])([^\s"',}]+))/gi, /[:=]/],
  [/(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])))(?:["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?(?:\r?\n|\r(?!\n)))*[ \t]*:(?!(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)))[ \t]*(?!(?:[&!][^\s"'`]*[ \t]+){3})(?!(?:[&!][^\s"'`]*[ \t]+)*[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?:(?<=:[ \t]*)(?:[&!][^\s"'`]*[ \t]+){1,2})?(?:"((?:[^"\\]|\\[\s\S]|\\$)+)|'((?:[^'\\]|\\[\s\S]|\\$)+)|`((?:[^`\\]|\\[\s\S]|\\$)+)|(?:Bearer|bearer|BEARER|Basic|basic|BASIC)\s+([A-Za-z0-9._~+/=-]+)|(?!(?:Bearer|bearer|BEARER|Basic|basic|BASIC)\b|\[redacted\]|[&!])([^\s"',}]+))/g, /Token|Secret|Pass|Key|Authorization/],
  [/(?:^|(?:\r?\n|\r(?!\n)))(?=[^\r\n]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)|[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))[ \t]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?=([ \t]*))\1[^\r\n#]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization))(?:["']?[ \t]*:|["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))\1[ \t]*:)(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?|\1[ \t]+[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)?(?:[ \t]+#[^\r\n]*)?[ \t]*)(?=[\r\n]|$))*(?:\r?\n|\r(?!\n))\1[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}(?![ \t&!#\r\n]|[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?=[\r\n]|$)|(?:"[^"\r\n]*"|'[^'\r\n]*'|(?:[^\r\n: \t]|[ \t]+(?=[^\r\n: \t#]))+)[ \t]*:(?:[ \t]|(?=[\r\n]|$)))(?:-[ \t]+(?=[^\s#]))?(?:"((?:[^"\\\r\n]|\\[^\r\n])+)"(?=[\s,}])|'((?:[^'\\\r\n]|\\[^\r\n])+)'(?=[\s,}])|`((?:[^`\\\r\n]|\\[^\r\n])+)`(?=[\s,}])|(?:Bearer|Basic)[ \t]+([A-Za-z0-9._~+/=-]+)(?=[^A-Za-z0-9._~+/=-])|(?!(?:Bearer|Basic)\b|\[redacted\])([^\s"',}]+)(?=[\s"',}]))(?=[^\r\n]*[\r\n])/gi, /:[ \t]*(?:[&!#][^\r\n]*)?(?:[\r\n]|$)/],
  [/(?:^|(?:\r?\n|\r(?!\n)))(?=[^\r\n]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$)|[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))[ \t]*:(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?=([ \t]*))\1[^\r\n#]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key))(?:["']?[ \t]*:|["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))\1[ \t]*:)(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?|\1[ \t]+[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)?(?:[ \t]+#[^\r\n]*)?[ \t]*)(?=[\r\n]|$))*(?:\r?\n|\r(?!\n))\1[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}(?![ \t&!#\r\n]|[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?=[\r\n]|$)|(?:"[^"\r\n]*"|'[^'\r\n]*'|(?:[^\r\n: \t]|[ \t]+(?=[^\r\n: \t#]))+)[ \t]*:(?:[ \t]|(?=[\r\n]|$)))(?:-[ \t]+(?=[^\s#]))?(?:"((?:[^"\\\r\n]|\\[^\r\n])+)"(?=[\s,}])|'((?:[^'\\\r\n]|\\[^\r\n])+)'(?=[\s,}])|`((?:[^`\\\r\n]|\\[^\r\n])+)`(?=[\s,}])|(?:Bearer|bearer|BEARER|Basic|basic|BASIC)[ \t]+([A-Za-z0-9._~+/=-]+)(?=[^A-Za-z0-9._~+/=-])|(?!(?:Bearer|bearer|BEARER|Basic|basic|BASIC)\b|\[redacted\])([^\s"',}]+)(?=[\s"',}]))(?=[^\r\n]*[\r\n])/g, /:[ \t]*(?:[&!#][^\r\n]*)?(?:[\r\n]|$)/],
  [/(?:^|(?:\r?\n|\r(?!\n)))(?=[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))[ \t]*:[ \t]*(?:[&!][^\s"'`]*[ \t]+){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?=([ \t]*))\1[^\r\n#]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])))(?:["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))\1[ \t]*:)(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?|\1[ \t]+[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)?(?:[ \t]+#[^\r\n]*)?[ \t]*)(?=[\r\n]|$))*(?:\r?\n|\r(?!\n))\1[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}(?![ \t&!#\r\n]|[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?=[\r\n]|$)|(?:"[^"\r\n]*"|'[^'\r\n]*'|(?:[^\r\n: \t]|[ \t]+(?=[^\r\n: \t#]))+)[ \t]*:(?:[ \t]|(?=[\r\n]|$)))(?:-[ \t]+(?=[^\s#]))?(?:"((?:[^"\\\r\n]|\\[^\r\n])+)"(?=[\s,}]|$)|'((?:[^'\\\r\n]|\\[^\r\n])+)'(?=[\s,}]|$)|`((?:[^`\\\r\n]|\\[^\r\n])+)`(?=[\s,}]|$)|(?:Bearer|Basic)[ \t]+([A-Za-z0-9._~+/=-]+)(?=[^A-Za-z0-9._~+/=-])|(?!(?:Bearer|Basic)\b|\[redacted\])([^\s"',}]+)(?=[\s"',}]|$))(?=[^\r\n]*[\r\n]|[ \t]*(?:#[^\r\n]*)?$)/gi, /\?[ \t]/, 'ended'],
  [/(?:^|(?:\r?\n|\r(?!\n)))(?=[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))[ \t]*:[ \t]*(?:[&!][^\s"'`]*[ \t]+){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:[\r\n]|$))(?=([ \t]*))\1[^\r\n#]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])))(?:["']?[ \t]*(?:#[^\r\n]*)?(?=[\r\n])(?<=(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*)(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))\1[ \t]*:)(?:[ \t]+[&!][^\s"'`]*){0,2}(?:[ \t]+#[^\r\n]*)?[ \t]*(?:(?:\r?\n|\r(?!\n))(?:[ \t]*(?:#[^\r\n]*)?|\1[ \t]+[&!][^\s"'`]*(?:[ \t]+[&!][^\s"'`]*)?(?:[ \t]+#[^\r\n]*)?[ \t]*)(?=[\r\n]|$))*(?:\r?\n|\r(?!\n))\1[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}(?![ \t&!#\r\n]|[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?=[\r\n]|$)|(?:"[^"\r\n]*"|'[^'\r\n]*'|(?:[^\r\n: \t]|[ \t]+(?=[^\r\n: \t#]))+)[ \t]*:(?:[ \t]|(?=[\r\n]|$)))(?:-[ \t]+(?=[^\s#]))?(?:"((?:[^"\\\r\n]|\\[^\r\n])+)"(?=[\s,}]|$)|'((?:[^'\\\r\n]|\\[^\r\n])+)'(?=[\s,}]|$)|`((?:[^`\\\r\n]|\\[^\r\n])+)`(?=[\s,}]|$)|(?:Bearer|bearer|BEARER|Basic|basic|BASIC)[ \t]+([A-Za-z0-9._~+/=-]+)(?=[^A-Za-z0-9._~+/=-])|(?!(?:Bearer|bearer|BEARER|Basic|basic|BASIC)\b|\[redacted\])([^\s"',}]+)(?=[\s"',}]|$))(?=[^\r\n]*[\r\n]|[ \t]*(?:#[^\r\n]*)?$)/g, /\?[ \t]/, 'ended'],
  [/(?:^|(?:\r?\n|\r(?!\n)))[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])))["']?[ \t]*(?:#[^\r\n]*)?(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))[ \t]*:[ \t]*(?:[&!][^\s"'`]*[ \t]+){0,2}[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:\r?\n|\r(?!\n))(?:(?:[ \t]*\r?(?=[\r\n]|$))*)[ \t]+([^\s"',}\r\n]+)/gi, /\?[ \t]/, 'ended'],
  [/(?:^|(?:\r?\n|\r(?!\n)))[ \t]*(?:-[ \t]+)?\?[ \t][^\r\n]*?(?=[A-Za-z0-9"'])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:(?:(?<!\\)"(?=[^"\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^"\r\n]*"|(?<!\\)'(?=[^'\r\n]*?(?<![A-Za-z0-9_-])(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?![A-Za-z0-9_-]))[^'\r\n]*')|(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n]){0,16}|(?<=(?:^|[\r\n])[ \t]{0,256}(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]{1,40}){0,2}[ \t]{0,256})(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)(?:(?!:[ \t])[^ \t"'#\r\n])*|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])))["']?[ \t]*(?:#[^\r\n]*)?(?:(?:\r?\n|\r(?!\n))[ \t]*(?:#[^\r\n]*)?)*(?:\r?\n|\r(?!\n))[ \t]*:[ \t]*(?:[&!][^\s"'`]*[ \t]+){0,2}[>|][-+0-9]*(?:[ \t]*#[^\r\n]*)?[ \t]*(?:\r?\n|\r(?!\n))(?:(?:[ \t]*\r?(?=[\r\n]|$))*)[ \t]+([^\s"',}\r\n]+)/g, /\?[ \t]/, 'ended'],
  [/(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization)\\"\s*:\s*\\"((?:[^\\"]|\\[^\\"]|\\\\(?:\\[\s\S]|\\$|[^\\]|$)|\\$)+)/gi, /\\"/],
  [/(?=[A-Za-z0-9])(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key)\\"\s*:\s*\\"((?:[^\\"]|\\[^\\"]|\\\\(?:\\[\s\S]|\\$|[^\\]|$)|\\$)+)/g, /\\"/],
  // The no-space tail is the value when no `:` line follows: remember its first token, so a
  // later bare occurrence is masked. A name line above a `:` line is S3's name tail.
  [/(^|[\r\n])((?:[ \t]*(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]+){0,2})(["'])(?=(?:(?!\3)[^\r\n:])*?(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z0-9]+[_-])*(?:passphrase|passw(?:or)?d|pass|secret(?:[_-](?:access[_-])?key)?|token|api[_-]?key|private[_-]?key|authorization))(?:(?!\3)[^\r\n:])*:(?![ \t])(?![^"'\r\n]*(?<!\\)\3[ \t]*(?::[^\r\n]*|#[^\r\n]*)?(?=[\r\n]|$)))(?![^\r\n]*[\r\n](?:[ \t]*(?:#[^\r\n]*)?[\r\n])*[ \t]*:)(["']?([^ \t\r\n"']+(?:'+[^ \t\r\n"']+)*))/g, /:/],
  [/(^|[\r\n])((?:[ \t]*(?:-[ \t]+)?\?[ \t](?:[&!][^\s"'`]{0,80}[ \t]+){0,2})(["'])(?=(?:(?!\3)[^\r\n:])*?(?:(?<![A-Za-z0-9_-])|(?<=(?<![A-Za-z0-9_-])-{1,2})|(?<=\\[nrt]))(?:[A-Za-z][a-z0-9]*(?![a-z0-9])(?:[A-Z][a-z0-9]*(?![a-z0-9]))*?(?:Token|Secret(?:(?:Access)?Key)?|Passw(?:or)?d|Passphrase|Pass|ApiKey|PrivateKey|Authorization)|(?:api|private)Key))(?:(?!\3)[^\r\n:])*:(?![ \t])(?![^"'\r\n]*(?<!\\)\3[ \t]*(?::[^\r\n]*|#[^\r\n]*)?(?=[\r\n]|$)))(?![^\r\n]*[\r\n](?:[ \t]*(?:#[^\r\n]*)?[\r\n])*[ \t]*:)(["']?([^ \t\r\n"']+(?:'+[^ \t\r\n"']+)*))/g, /:/],
]

// #590: the explicit-key guard in the name rules scans a bounded quote tail at every name. On a
// text with no `? name` line the guard can never fail, so dropping it is exact; the rule is
// compiled with and without it and chosen per text, keeping an unclosed quote run linear (#172).
const EXPLICIT_KEY = /(?:^|[\r\n])[ \t]*(?:-[ \t]+)?\?[ \t]/
const ANY_EXPLICIT_KEY = /\?[ \t]/
const GUARD_START = '(?!(?<=(?:^|[\\r\\n])[ \\t]{0,160}'
const KEY_TAIL_START = String.raw`|(?<=(?:^|[\r\n])[ \t]{0,256}`
const QUOTE_TAIL = String.raw`|(?:"[^"\r\n]{241,}|'[^'\r\n]{241,}`
  + String.raw`|"[^"\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n])`
  + String.raw`|'[^'\r\n]{0,}(?=[ \t]{0,64}(?:#[^\r\n]*)?[\r\n]))`
function withoutGuard(source) {
  let out = source
  for (let at = out.indexOf(GUARD_START); at >= 0; at = out.indexOf(GUARD_START)) {
    let depth = 0
    let end = at
    for (; end < out.length; end++) {
      if (out[end] === '\\') { end++; continue }
      if (out[end] === '(') depth++
      else if (out[end] === ')' && --depth === 0) { end++; break }
    }
    out = out.slice(0, at) + out.slice(end)
  }
  for (let at = out.indexOf(KEY_TAIL_START); at >= 0; at = out.indexOf(KEY_TAIL_START)) {
    const next = out.indexOf(QUOTE_TAIL, at)
    if (next < 0) break
    out = out.slice(0, at) + out.slice(next)
  }
  return out.split(QUOTE_TAIL).join('')
}
function nameRule(pattern, replace) {
  const plain = new RegExp(withoutGuard(pattern.source), pattern.flags)
  return (text) => (EXPLICIT_KEY.test(text)
    ? text.replace(pattern, replace)
    : text.replace(plain, replace))
}
for (const rule of SECRET_TEXT) {
  if (typeof rule[0] !== 'function' &&
    (rule[0].source.includes(GUARD_START) || rule[0].source.includes(QUOTE_TAIL))) {
    rule[0] = nameRule(rule[0], rule[1])
    rule[1] = null
  }
}
for (let i = 0; i < SECRET_VALUES.length; i++) {
  const entry = SECRET_VALUES[i]
  if (!Array.isArray(entry) ||
    !(entry[0].source.includes(GUARD_START) || entry[0].source.includes(QUOTE_TAIL))) continue
  const gate = entry[1]
  // A rule whose own gate already requires `? ` has no plain variant to add: it skips every
  // `?`-free text already.
  if (gate instanceof RegExp && gate.source === ANY_EXPLICIT_KEY.source) continue
  const live = (t) => !gate || gate.test(t)
  const plainGate = { test: (t) => live(t) && !ANY_EXPLICIT_KEY.test(t) }
  const guardedGate = { test: (t) => live(t) && ANY_EXPLICIT_KEY.test(t) }
  const plain = new RegExp(withoutGuard(entry[0].source), entry[0].flags)
  SECRET_VALUES.splice(i, 0, [plain, plainGate, ...entry.slice(2)])
  entry[1] = guardedGate
  i++
}

export function secretsIn(s, open = false) {
  const out = new Set()
  const text = String(s)
  const normalized = nameText(text)
  for (const t of new Set([text, normalized])) for (const entry of SECRET_VALUES) {
    const [re, gate, whole] = Array.isArray(entry) ? entry : [entry]
    if (gate && !gate.test(t)) continue
    for (const m of t.matchAll(re)) {
      // An explicit-key entry is remembered once its value's line has ended, even in a growing
      // buffer (#406 FAIL 2, NARROW 4): the value ends at the end of the text, still arriving, or
      // before it, where a line break, a blank or a comment closed it, as later-line entries are.
      if (whole === 'ended' && open && m.index + m[0].length >= t.length) continue
      const v = m.slice(1).findLast((x) => x !== undefined) // the quoted or the bare value, after a continued key's indent
      if (v && v.length >= 4 && v !== REDACTED) out.add(v)
    }
  }
  // The malformed explicit-key fallback hides a block before the ordinary value rules run.
  // A following colon selects its value; earlier indented text is part of the opaque name.
  if (EXPLICIT_KEY.test(normalized)) {
    const keys = /(^|\r\n|\r(?!\n)|\n)([ \t]*)(-[ \t]+)?\?[ \t]+(?:[&!][^\s"'`]*[ \t]+){0,2}("(?:[^"\\\r\n]|\\[^\r\n])*"|'(?:[^'\r\n]|'')*')([^\r\n]*)/g
    for (const key of normalized.matchAll(keys)) {
      if (/^[ \t]*(?:#[^\r\n]*|:[^\r\n]*)?$/.test(key[5]) || !NAME_STEM.test(key[4] + key[5])) continue
      const indent = key[2] + (key[3] ? ' '.repeat(key[3].length) : '')
      let from = key.index + key[0].length
      let end = yamlBlockEnd(normalized, from, indent)
      const colon = /^(?:(?:\r\n|\r(?!\n)|\n)[ \t]*(?:#[^\r\n]*)?)*(?:\r\n|\r(?!\n)|\n)([ \t]*):([^\r\n]*)/.exec(normalized.slice(end))
      if (colon && colon[1] === indent) {
        const lineEnd = end + colon[0].length
        from = lineEnd - colon[2].length
        end = yamlBlockEnd(normalized, lineEnd, indent)
      }
      for (const line of normalized.slice(from, end).matchAll(/[^\r\n]+/g)) {
        if (open && from + line.index + line[0].length >= normalized.length) continue
        const raw = line[0].trimStart()
        if (!raw || /^[#&!|>]/.test(raw)) continue
        const child = /^(?:"(?:[^"\\]|\\.)*"|'(?:[^']|'')*'|[^"'#:][^:]*):(?:[ \t]+|$)(.*)$/.exec(raw)
        const value = child ? child[1].trimStart() : raw
        if (/^[&!|>]/.test(value)) continue
        const token = /^(?:"((?:[^"\\]|\\.)*)"|'((?:[^']|'')*)'|([^ \t"'#]+))/.exec(value)
        const v = token?.slice(1).find((part) => part !== undefined)
        if (v && v.length >= 4 && v !== REDACTED) out.add(v)
      }
    }
  }
  for (const { value, ended } of explicitKeyHits(text)) {
    if (open && ended >= text.length) continue
    if (value && value.length >= 4 && value !== REDACTED) out.add(value)
  }
  return [...out]
}
