// The hidden-character vectors every layer shares, copied from the wish (Group 3 Interfaces,
// "Hidden characters (one rule, three layers)"). The comment keeps the wish's own lines; the
// arrays hold the same strings as escapes, so no invisible character sits in the source.
//   Allowed: ❤\uFE0F (U+2764 U+FE0F); 👨\u200D💻 (U+1F468 U+200D U+1F4BB);
//            👩\u200D❤\uFE0F\u200D👨 (U+1F469 U+200D U+2764 U+FE0F U+200D U+1F468); ok 👍🏽 (no ignorable);
//            👩🏽\u200D💻 (U+1F469 U+1F3FD U+200D U+1F4BB).
//   Refused: \u200D alone; a\u200Db (ZWJ between letters); a\uFE0F (VS16 after a letter);
//            👨\u200Da (ZWJ before a letter); \uFE0F👍 (VS16 first); a\u200Bb (ZWSP);
//            file\u202Etxt.exe (RLO); run\U000E0074 (tag); a\u061Cb (ALM);
//            a\u200D👍 (ZWJ after a letter, before an emoji).
export const ALLOWED = ['❤\uFE0F', '\u{1F468}\u200D\u{1F4BB}', '\u{1F469}\u200D❤\uFE0F\u200D\u{1F468}', 'ok \u{1F44D}\u{1F3FD}', '\u{1F469}\u{1F3FD}\u200D\u{1F4BB}']
export const REFUSED = ['\u200D', 'a\u200Db', 'a\uFE0F', '\u{1F468}\u200Da', '\uFE0F\u{1F44D}', 'a\u200Bb', 'file\u202Etxt.exe', 'run\u{E0074}', 'a\u061Cb', 'a\u200D\u{1F44D}']
