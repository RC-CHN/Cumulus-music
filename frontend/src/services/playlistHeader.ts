/** The recovered PC formatter uses the local calendar date, not UTC. */
export function playlistDate(timestamp: number | undefined): string | null {
  if (!Number.isSafeInteger(timestamp) || timestamp == null || timestamp <= 0) return null;
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime())) return null;
  return [date.getFullYear(), date.getMonth() + 1, date.getDate()]
    .map((part, index) => String(part).padStart(index === 0 ? 4 : 2, '0'))
    .join('-');
}
