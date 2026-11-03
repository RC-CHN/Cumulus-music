const compactCount = new Intl.NumberFormat('zh-CN', {
  notation: 'compact',
  maximumFractionDigits: 1,
});
const fullCount = new Intl.NumberFormat('zh-CN');

export function playlistCoverCount(count: number | null | undefined, privateList: boolean) {
  if (privateList || count == null || !Number.isSafeInteger(count) || count < 1) return null;
  return { label: `播放次数：${fullCount.format(count)}`, text: compactCount.format(count) };
}

/** The recovered PC formatter uses the local calendar date, not UTC. */
export function playlistDate(timestamp: number | undefined): string | null {
  if (!Number.isSafeInteger(timestamp) || timestamp == null || timestamp <= 0) return null;
  const date = new Date(timestamp);
  if (!Number.isFinite(date.getTime())) return null;
  return [date.getFullYear(), date.getMonth() + 1, date.getDate()]
    .map((part, index) => String(part).padStart(index === 0 ? 4 : 2, '0'))
    .join('-');
}
