interface SearchTextPart {
  text: string;
  match: boolean;
}

/** Preserve the displayed text, including the exact Unicode offsets of each match. */
export function splitSearchMatch(text: string, keyword: string): SearchTextPart[] {
  const query = keyword.trim().replace(/\s+/g, ' ');
  if (!query || query.length > 512) return [{ text, match: false }];
  const literal = query.replace(/[.*+?^$(){}|[\]\\]/g, '\\$&');
  const pattern = new RegExp(literal, 'giu');
  const parts: SearchTextPart[] = [];
  let at = 0;
  let count = 0;
  for (const found of text.matchAll(pattern)) {
    if (count++ >= 100) break;
    if (found.index > at) parts.push({ text: text.slice(at, found.index), match: false });
    parts.push({ text: found[0], match: true });
    at = found.index + found[0].length;
  }
  if (at < text.length || !parts.length) parts.push({ text: text.slice(at), match: false });
  return parts;
}
