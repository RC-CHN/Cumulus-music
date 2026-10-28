// Official M/123 b: whitespace/case, full-width characters, then the recovered
// one-character traditional/simplified table. This is not general transliteration.
import table from './keywordMap.json' with { type: 'json' };
const characters = new Map<string, string>();
for (let i = 0; i < table.traditional.length; i++) {
  const from = table.traditional[i]!;
  if (!characters.has(from)) characters.set(from, table.simplified[i]!);
}
export function normalizeKeyword(value: string): string {
  const width = value
    .trim()
    .replace(/\s+/g, ' ')
    .toLowerCase()
    .replace(/[\uff00-\uffff]/g, (character) =>
      String.fromCharCode(character.charCodeAt(0) - 65248),
    );
  return [...width].map((character) => characters.get(character) ?? character).join('');
}
