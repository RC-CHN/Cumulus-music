import type { TrackPage, TrackView } from './contracts';

export type PlaylistDurationOrder = 'original' | 'ascending' | 'descending';
export interface PlaylistDurationSource {
  revision: string;
  local_revision: string | null;
  cached_at_ms: number | null;
}
export interface PlaylistDurationScope {
  receipt: string;
  order: Exclude<PlaylistDurationOrder, 'original'>;
}
export interface PlaylistDurationPage {
  receipt: string;
  page: TrackPage;
  ascending: number[];
  descending: number[];
}

export function nextPlaylistDurationOrder(order: PlaylistDurationOrder): PlaylistDurationOrder {
  return order === 'original' ? 'ascending' : order === 'ascending' ? 'descending' : 'original';
}

export function playlistDurationSource(id: string, shown: TrackPage | undefined) {
  if (!shown?.revision || shown.playlist?.id !== id || shown.playlist.favorite) return;
  return {
    revision: shown.revision,
    local_revision: shown.playlistLocal?.revision ?? null,
    cached_at_ms: shown.playlistOffline?.cachedAtMs ?? null,
  };
}

/** Preserve the natural ordinals used by typed row actions. The native
 * complete receipt supplies the permutation; text search filters that order. */
export function sortedPlaylistRows(
  visible: { tracks: TrackView[]; positions: number[] | null },
  order: number[] | undefined,
) {
  if (!order) return visible;
  const filtered = visible.positions && new Map(visible.positions.map((pos, i) => [pos, i]));
  const tracks: TrackView[] = [];
  const positions: number[] = [];
  for (const ordinal of order) {
    const index = filtered ? filtered.get(ordinal) : ordinal;
    if (index !== undefined && visible.tracks[index]) {
      positions.push(ordinal);
      tracks.push(visible.tracks[index]);
    }
  }
  return { tracks, positions };
}
