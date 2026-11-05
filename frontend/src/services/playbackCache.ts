/** Device-wide native playback-byte cache. Never pass a filesystem path to a command. */
export interface PlaybackCacheStatus {
  revision: string;
  directory: string;
  capacityBytes: number;
  minCapacityBytes: number;
  maxCapacityBytes: number;
  capacityStepBytes: number;
  occupiedBytes: number;
  reservedBytes: number;
  reclaimableBytes: number;
  overCapacity: boolean;
}

export interface PlaybackCacheClient {
  playbackCacheState(): Promise<PlaybackCacheStatus | null>;
  playbackCacheSetCapacity(
    expectedRevision: string,
    capacityBytes: number,
  ): Promise<PlaybackCacheStatus>;
  playbackCacheClear(expectedRevision: string): Promise<PlaybackCacheStatus>;
}

const gibibyte = 1024 ** 3;
const mebibyte = 1024 ** 2;
const display = new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 2 });

export function playbackCacheQuota(bytes: number, spoken = false): string {
  return bytes < gibibyte
    ? `${display.format(bytes / mebibyte)}${spoken ? ' MiB' : 'M'}`
    : `${display.format(bytes / gibibyte)}${spoken ? ' GiB' : 'G'}`;
}

export function playbackCacheUsage(bytes: number): string {
  if (bytes < 1024) return `${display.format(bytes)} 字节`;
  if (bytes < mebibyte) return `${display.format(bytes / 1024)} KiB`;
  if (bytes < gibibyte) return `${display.format(bytes / mebibyte)} MiB`;
  return `${display.format(bytes / gibibyte)} GiB`;
}

export function playbackCacheCapacityAllowed(
  view: PlaybackCacheStatus,
  capacityBytes: number,
): boolean {
  return (
    Number.isSafeInteger(capacityBytes) &&
    Number.isSafeInteger(view.capacityStepBytes) &&
    view.capacityStepBytes > 0 &&
    capacityBytes >= view.minCapacityBytes &&
    capacityBytes <= view.maxCapacityBytes &&
    (capacityBytes - view.minCapacityBytes) % view.capacityStepBytes === 0
  );
}
