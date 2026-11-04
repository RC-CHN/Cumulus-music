const compactCount = new Intl.NumberFormat('zh-CN', {
  notation: 'compact',
  maximumFractionDigits: 1,
});
const fullCount = new Intl.NumberFormat('zh-CN');

export function playlistCoverCount(count: number | null | undefined, privateList: boolean) {
  if (privateList || count == null || !Number.isSafeInteger(count) || count < 1) return null;
  return { label: `播放次数：${fullCount.format(count)}`, text: compactCount.format(count) };
}

/** PC table rows use the default M/32.f compact threshold (100,000). */
export function playlistTableCount(count: number | null | undefined, privateList: boolean) {
  if (privateList || count == null || !Number.isSafeInteger(count) || count < 0) return null;
  const text =
    count > 100_000_000
      ? `${Math.floor(count / 10_000_000) / 10}亿`
      : count > 100_000
        ? `${Math.floor(count / 1_000) / 10}万`
        : String(count);
  return { label: `播放次数：${fullCount.format(count)}`, text };
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
