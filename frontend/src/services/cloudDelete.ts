import type { CloudTrack } from './cloud.ts';

export interface CloudDeleteTarget {
  epoch: string;
  ids: string[];
  titles: string[];
  revision: string;
  kinds?: CloudTrack['deletionKind'][];
}

export function captureCloudDeleteTarget(target: CloudDeleteTarget): CloudDeleteTarget {
  return {
    ...target,
    ids: [...target.ids],
    titles: [...target.titles],
    kinds: target.kinds ? [...target.kinds] : undefined,
  };
}

/** Capture the exact review context once, before any refresh can replace rows. */
export function cloudDeleteTarget(
  epoch: string,
  revision: string,
  tracks: readonly CloudTrack[],
): CloudDeleteTarget {
  return {
    epoch,
    revision,
    ids: tracks.map((row) => row.track.id),
    titles: tracks.map((row) => row.track.name),
    kinds: tracks.map((row) => row.deletionKind ?? 'unknown'),
  };
}

/** Official action gives private warnings priority over off-store warnings. */
export function cloudDeleteWarning(target: CloudDeleteTarget): string {
  const kinds = target.kinds ?? [];
  return kinds.includes('private')
    ? '从网盘删除文件后，其他端将无法同步，确认删除？'
    : kinds.includes('off_store')
      ? '从网盘删除文件后，将无法再试听下载，确认删除？'
      : kinds.length !== target.ids.length || kinds.some((kind) => !kind || kind === 'unknown')
        ? '部分歌曲类型尚未核实，删除后可能无法再次同步、试听或下载。确认删除？'
        : '确定将选中的歌曲从网盘删除？';
}
