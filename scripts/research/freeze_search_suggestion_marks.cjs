// Manual, sealed oracle for the recovered 3.1.42.205594 factories.
// No source bundles, dictionary strings, services or browser globals are used
// by production or automatic tests. Run only with explicitly supplied evidence.
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

if (process.argv.length !== 4 || process.version !== 'v24.16.0') {
  throw new Error('usage (Node 24.16.0): freeze_search_suggestion_marks.cjs MODULE_DIR NEW_OUTPUT_DIR');
}
const input = path.resolve(process.argv[2]);
const output = path.resolve(process.argv[3]);
assert(!fs.existsSync(output), 'output directory must be new');
const pins = {
  87: 'cf48fb051c00eda4d9fdb6a0ebc3ab29ac8e904000943b47cfa7d6d01d3f5c56',
  123: '49c9a38bbf1fd1ebbe9808210034fa897176f9011607c0a8bd374ac9240c3015',
  623: 'bdefea2ef4fadab00afef5371b38957045629748a081d1b0ffe2748c925989aa',
};
const sha = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
const modules = Object.fromEntries(Object.entries(pins).map(([id, digest]) => {
  const bytes = fs.readFileSync(path.join(input, id + '.js'));
  assert.equal(sha(bytes), digest, 'recovered factory hash ' + id);
  return [id, bytes.toString('utf8')];
}));

const ctx = vm.createContext({}, {codeGeneration: {strings: false, wasm: false}});
const exportsById = {
  1: {Fragment: Symbol('Fragment'), jsx: (tag, props) => ({tag, props})},
  2: {b: {error() { throw new Error('unexpected original matcher error'); }}},
};
function requireFactory(id) {
  assert(Object.hasOwn(exportsById, id), 'unexpected module ' + id);
  return exportsById[id];
}
requireFactory.d = (target, name, get) => Object.defineProperty(target, name, {get});
for (const id of [123, 87]) {
  const factory = new vm.Script(modules[id], {filename: 'M' + id}).runInContext(ctx, {timeout: 1000});
  const exports = {};
  factory({}, exports, requireFactory);
  exportsById[id] = exports;
}
const normalize = exportsById[123].b;
const rawParts = exportsById[123].d;
const markText = exportsById[87].b;

const [src, dst] = modules[123].split('\n').slice(51, 53).map((line) => {
  const literal = line.match(/"(?:[^"\\]|\\.)*"/)?.[0];
  assert(literal, 'paired string source anchor moved');
  return JSON.parse(literal);
});
assert.equal(src.length, 2359);
assert.equal(dst.length, src.length);
assert.equal(Array.from(src).length, src.length);
assert.equal(Array.from(dst).length, dst.length);
const glyphs = new Map();
const duplicates = [];
for (let i = 0; i < src.length; i++) {
  const from = src.charCodeAt(i);
  const to = dst.charCodeAt(i);
  assert(from < 0xd800 || from > 0xdfff);
  assert(to < 0xd800 || to > 0xdfff);
  if (glyphs.has(from)) duplicates.push([from, glyphs.get(from), to]);
  else glyphs.set(from, to);
}
assert.equal(glyphs.size, 2351);
assert.equal(duplicates.length, 8);
assert(duplicates.every(([, first, ignored]) => first !== ignored));
const table = Buffer.alloc(glyphs.size * 4);
let offset = 0;
for (const [from, to] of [...glyphs].sort(([a], [b]) => a - b)) {
  assert.equal(normalize(String.fromCharCode(from)), String.fromCharCode(to));
  table.writeUInt16LE(from, offset);
  table.writeUInt16LE(to, offset + 2);
  offset += 4;
}

const inputs = [
  ['simplified_exact', '音乐与乐队', '音乐'],
  ['traditional_query_to_simplified', '音樂：音乐 / 樂隊 乐队', '音樂 樂隊'],
  ['traditional_label_not_normalized', '音樂 / 音乐', '音乐'],
  ['first_duplicate_wins', String.fromCharCode(duplicates[0][2], duplicates[0][1]), String.fromCharCode(duplicates[0][0])],
  ['case_preserved', 'cat catalog CAT', 'CaT'],
  ['fullwidth_query_only', 'ＡＢＣ ABC aBc', 'ＡＢＣ'],
  ['width_space_creates_first_separator', 'a b c / b / C', 'a\uff00b c'],
  ['broad_width_range_not_nfkc', 'ｶ \u0096', 'ｶ'],
  ['three_tokens_first_separator_only', 'A b c B\tC b A abc', ' A\tB  C '],
  ['no_word_segmentation', '甲乙丙 乙丙', '乙'],
  ['left_alternative_wins_overlap', 'aaaaa', 'aa a'],
  ['reversed_alternative_wins_overlap', 'aaaaa', 'a aa'],
  ['whitespace_normalized_only_query', '\ufeff  a\u00a0b A B b\r\nc \ufeff', 'a\u3000b c'],
  ['nel_is_not_js_whitespace', '\u0085x\u0085', '\u0085x'],
  ['trim_label_and_blank_query', '  preserved \u3000', ' \ufeff\t'],
  ['empty_label_blank_query', '\u3000', ''],
  ['empty_label_nonempty_query', '\u3000', 'music'],
  ['regex_tokens_literal', 'a.b[?]! | ? \\ aXb[?]!', 'a.b[?]! | ? \\'],
  ['html_like_stays_text', '<img onerror=fixture> & <IMG>', '<img'],
  ['legacy_case_no_long_s', 'ſsS', 's'],
  ['legacy_case_no_kelvin_label', 'KkK', 'k'],
  ['query_lower_kelvin_does_map', 'KkK', 'K'],
  ['multi_unit_uppercase_not_casefold', 'ßẞSS', 'ẞ'],
  ['query_lower_can_expand', 'İ i\u0307 I i', 'İ'],
  ['greek_sigma', 'ΟΣ οσ ος', 'ΟΣ'],
  ['astral_not_folded_by_non_u_regex', '𐐀𐐨', '𐐀'],
  ['valid_utf16_pair_offsets', '🙂歌🙂樂乐', '🙂 樂'],
  ['100_ascii_matches_tail', 'A'.repeat(103), 'a'],
  ['100_astral_matches_tail', '🔊'.repeat(102) + '尾', '🔊'],
  ['empty_after_width_100_empty_matches', 'safe', '\uff00', 'plain_for_empty_pattern'],
  ['largest_current_keyword', 'x'.repeat(256), 'X'.repeat(256)],
  ['query_above_current_projection_limit', 'a'.repeat(257), 'a'.repeat(257), 'plain_for_input_bound'],
  ['label_above_current_projection_limit', 'x'.repeat(257), 'x', 'plain_for_input_bound'],
];
function parts(label, keyword) {
  const raw = Array.from(rawParts(label, keyword), ({value, isMark}) => ({text: value, match: isMark}));
  const node = markText({children: label, keyword, markClassName: 'test-mark'});
  assert.equal(node.tag, exportsById[1].Fragment);
  const rendered = Array.from(node.props.children, (child) => {
    if (typeof child === 'string') return {text: child, match: false};
    assert.equal(child.tag, 'span');
    assert.equal(child.props.className, 'test-mark');
    return {text: child.props.children, match: true};
  });
  assert.deepEqual(raw, rendered);
  return raw;
}
function offsets(items) {
  let at = 0;
  return items.flatMap((p) => {
    const start = at;
    at += p.text.length;
    return p.match ? [[start, at]] : [];
  });
}
const vectors = inputs.map(([name, label, keyword, scope = 'exact']) => {
  const originalParts = parts(label, keyword);
  return {name, label, keyword, scope, originalParts, originalMatchUtf16: offsets(originalParts)};
});
const units = (s) => Array.from({length: s.length}, (_, i) => s.charCodeAt(i));
const unsupportedUtf16 = [
  ['high_half_inside_label', '🙂x', '\ud83d'],
  ['low_half_inside_label', '🙂x', '\ude42'],
  ['lone_surrogate_label', 'x\ud83dy', '\ud83d'],
].map(([name, label, keyword]) => {
  const originalParts = parts(label, keyword);
  return {
    name, labelUnits: units(label), queryUnits: units(keyword),
    originalParts: originalParts.map((part) => ({units: units(part.text), match: part.match})),
    originalMatchUtf16: offsets(originalParts),
  };
});
const bytes = Buffer.from(JSON.stringify({
  schema: 'cumulus-search-suggestion-marks-original-v1', vectors, unsupportedUtf16,
}, null, 2) + '\n');
const manifest = {
  schema: 'cumulus-search-suggestion-marks-original-v1', factorySha256: pins,
  source: 'Official PC 3.1.42.205594 recovered hybrid_app chunk, M123/M87; M623 static route only',
  generatorSha256: sha(fs.readFileSync(__filename)),
  oracleNode: process.version, oracleUnicode: process.versions.unicode,
  table: {
    sha256: sha(table), bytes: table.length, encodedAs: 'sorted u16 little-endian source/target pairs',
    sourcePositions: src.length, uniquePairs: glyphs.size, ignoredDuplicates: duplicates.length,
    originalNormalizationComparisons: glyphs.size,
  },
  vectors: {sha256: sha(bytes), validScalarCases: vectors.length, isolatedSurrogateCases: unsupportedUtf16.length},
};
fs.mkdirSync(output);
fs.writeFileSync(path.join(output, 'glyph-pairs.bin'), table, {flag: 'wx'});
fs.writeFileSync(path.join(output, 'vectors.json'), bytes, {flag: 'wx'});
fs.writeFileSync(path.join(output, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', {flag: 'wx'});
console.log(JSON.stringify(manifest));
