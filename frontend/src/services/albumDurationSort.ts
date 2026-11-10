import type { TrackView } from './contracts.ts';

export type AlbumDurationOrder = 'original' | 'ascending' | 'descending';
export type AlbumPopularityOrder = 'original' | 'popularity_ascending' | 'popularity_descending';
export type AlbumTrackOrder = AlbumDurationOrder | AlbumPopularityOrder;

export interface AlbumPlaybackScope {
  id: string;
  revision: string;
  order: AlbumTrackOrder;
}

export interface AlbumPlaybackRow {
  visible_ordinals: number[] | null;
  track_index: number;
  track_id: string;
}

export function nextAlbumDurationOrder(order: AlbumTrackOrder): AlbumDurationOrder {
  if (order === 'ascending') return 'descending';
  if (order === 'descending') return 'original';
  return 'ascending';
}

export function nextAlbumPopularityOrder(order: AlbumTrackOrder): AlbumPopularityOrder {
  if (order === 'popularity_descending') return 'popularity_ascending';
  if (order === 'popularity_ascending') return 'original';
  return 'popularity_descending';
}

/** Derive visible order separately from original album ordinals. Ties keep
 * original order in either direction (official M/106 and M/470). */
export function sortAlbumTracks(
  visible: { tracks: TrackView[]; positions: number[] | null },
  order: AlbumTrackOrder,
) {
  if (order === 'original') return visible;
  const sign = order === 'ascending' || order === 'popularity_ascending' ? 1 : -1;
  const score = (track: TrackView) =>
    order === 'ascending' || order === 'descending'
      ? track.durationMs
      : (track.albumPopularity ?? 0);
  const rows = visible.tracks
    .map((track, index) => ({ track, ordinal: visible.positions?.[index] ?? index }))
    .sort((a, b) => sign * (score(a.track) - score(b.track)));
  return {
    tracks: rows.map(({ track }) => track),
    positions: rows.map(({ ordinal }) => ordinal),
  };
}
