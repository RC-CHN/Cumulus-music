export function formatPlaybackCommentCount(count: number) {
  if (!Number.isSafeInteger(count) || count <= 0) return '';
  if (count <= 999) return String(count);
  if (count <= 9_999) return '999+';
  if (count <= 99_999) return '1w+';
  if (count <= 999_999) return '10w+';
  return '100w+';
}
