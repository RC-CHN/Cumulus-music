import type { TrackPage, TrackView } from './contracts';
import type { PlaySelection } from './player';

function available(page: TrackPage, track: TrackView, downloads: ReadonlySet<string>): boolean {
  if (track.missing) return false;
  const local = page.playlistLocal;
  return track.local
    ? !!local && Object.hasOwn(local.rows, track.id)
    : !track.voiceId && /^(?=[0-9]*[1-9])[0-9]+$/.test(track.id) && downloads.has(track.id);
}

/** Native hints are for the displayed page only; the actor rechecks file and rights. */
export function offlinePlaylistPlayableIds(page: TrackPage): Set<string> {
  const offline = page.playlistOffline;
  const ids = new Set<string>();
  if (!offline) return ids;
  const downloads = new Set(offline.downloadIds);
  for (const track of page.tracks) if (available(page, track, downloads)) ids.add(track.id);
  return ids;
}

export function offlinePlaylistSelection(
  id: string,
  page: TrackPage,
  track?: TrackView,
  doubleClick = false,
): Extract<PlaySelection, { kind: 'offline_playlist' }> | undefined {
  const local = page.playlistLocal;
  const offline = page.playlistOffline;
  if (!offline || page.playlist?.id !== id || !local || (!track && page.total === 0)) return;
  if (
    track &&
    (!page.tracks.includes(track) || !available(page, track, new Set(offline.downloadIds)))
  )
    return;
  return {
    kind: 'offline_playlist',
    id,
    revision: local.revision,
    cached_at_ms: offline.cachedAtMs,
    start_id: track?.id ?? null,
    ...(track && !track.local ? { start_local: false } : {}),
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
