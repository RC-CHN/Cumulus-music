import { currentCatalogTrack, type PlayerView } from './player.ts';

export interface SongSpeedMoreTarget {
  key: string;
  label: string;
}

export function songSpeedMoreTarget(
  view: PlayerView | undefined,
  epoch: string,
  ready: boolean,
  togetherActive: boolean,
): SongSpeedMoreTarget | undefined {
  if (
    !ready ||
    togetherActive ||
    view?.epoch !== epoch ||
    view.speedScope !== 'song' ||
    !view.songSpeed?.enabled ||
    !view.entryId
  )
    return;
  const song = currentCatalogTrack(view);
  if (!song) return;
  return {
    key: JSON.stringify([epoch, song.playId, view.entryId, view.songSpeed.revision]),
    label: view.speed === 1 ? '播放倍速' : `播放倍速：${view.speed}X`,
  };
}

/** M3085 anchors the 218px panel at the More button's right edge and 246px above it. */
export function songSpeedMoreBounds(
  anchor: Pick<DOMRectReadOnly, 'right' | 'top'>,
  viewportWidth: number,
  viewportHeight: number,
) {
  const width = Math.min(218, Math.max(0, viewportWidth - 16));
  const top = Math.max(8, Math.min(anchor.top - 246, viewportHeight - 8));
  return {
    left: Math.max(8, Math.min(anchor.right - width, viewportWidth - width - 8)),
    top,
    width,
    height: Math.max(0, Math.min(246, anchor.top - top, viewportHeight - top - 8)),
  };
}
