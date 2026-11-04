import type { TrackView } from './contracts';

export type TrackPlayGesture = 'double_click';
export interface TrackCurrentToggle {
  trackId: string;
  epoch: string;
  playId: string;
  queueRevision: string;
  pause: boolean;
}

// UI-only request: playback retains the originating list and its ordering.
export interface TrackMenuRequest {
  track: TrackView;
  event: MouseEvent | KeyboardEvent;
  play: () => void;
  nextOrigin?: 'search';
}
export function songLink(track: TrackView) {
  return !track.local && /^[1-9][0-9]*$/.test(track.id)
    ? `https://music.163.com/song?id=${track.id}`
    : undefined;
}
