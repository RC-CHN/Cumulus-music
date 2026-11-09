import type { TrackPage, TrackView } from './contracts';
import type { PlaySelection } from './player';

/** The offline marker labels a read projection, not a catalogue audio grant. */
export function offlinePlaylistSelection(
  id: string,
  page: TrackPage,
  track?: TrackView,
  doubleClick = false,
): Extract<PlaySelection, { kind: 'offline_playlist' }> | undefined {
  const local = page.playlistLocal;
  if (!page.playlistOffline || page.playlist?.id !== id || !local) return;
  if (
    track &&
    (!track.local ||
      track.missing ||
      !page.tracks.includes(track) ||
      !Object.hasOwn(local.rows, track.id))
  )
    return;
  return {
    kind: 'offline_playlist',
    id,
    revision: local.revision,
    start_id: track?.id ?? null,
    ...(doubleClick && track ? { double_click: true } : {}),
  };
}

export function samePlaylistReadSource(a: TrackPage, b: TrackPage): boolean {
  if (!a.playlistOffline && !b.playlistOffline) return true;
  return (
    !!a.playlistOffline &&
    !!b.playlistOffline &&
    a.playlistOffline.cachedAtMs === b.playlistOffline.cachedAtMs &&
    a.revision === b.revision &&
    a.playlistLocal?.revision === b.playlistLocal?.revision
  );
}
