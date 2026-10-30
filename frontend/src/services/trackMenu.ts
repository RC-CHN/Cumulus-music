import type { TrackView } from './contracts';

// UI-only request: playback retains the originating list and its ordering.
export interface TrackMenuRequest {
  track: TrackView;
  event: MouseEvent | KeyboardEvent;
  play: () => void;
}
export function songLink(track: TrackView) {
  return !track.local && /^[1-9][0-9]*$/.test(track.id)
    ? `https://music.163.com/song?id=${track.id}`
    : undefined;
}
