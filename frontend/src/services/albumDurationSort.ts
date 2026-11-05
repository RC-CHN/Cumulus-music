import type { TrackView } from './contracts.ts';

export type AlbumDurationOrder = 'original' | 'ascending' | 'descending';

export interface AlbumPlaybackScope {
  id: string;
  revision: string;
  order: AlbumDurationOrder;
}

export interface AlbumPlaybackRow {
  visible_ordinals: number[] | null;
  track_index: number;
  track_id: string;
}

export function nextAlbumDurationOrder(order: AlbumDurationOrder): AlbumDurationOrder {
  if (order === 'original') return 'ascending';
  if (order === 'ascending') return 'descending';
  return 'original';
}

/** Derive visible order separately from original album ordinals. Equal lengths
 * keep original order in either direction (official M/106 and M/470). */
export function sortAlbumDuration(
  visible: { tracks: TrackView[]; positions: number[] | null },
  order: AlbumDurationOrder,
) {
  if (order === 'original') return visible;
  const sign = order === 'ascending' ? 1 : -1;
  const rows = visible.tracks
    .map((track, index) => ({ track, ordinal: visible.positions?.[index] ?? index }))
    .sort((a, b) => sign * (a.track.durationMs - b.track.durationMs));
  return {
    tracks: rows.map(({ track }) => track),
    positions: rows.map(({ ordinal }) => ordinal),
  };
}
