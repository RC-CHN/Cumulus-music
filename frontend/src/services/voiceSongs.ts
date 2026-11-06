import type { TrackView } from './contracts.ts';
import type { PlayerView, Quality } from './player.ts';

export interface VoiceSongsScope {
  epoch: string;
  playId: string;
  voiceId: string;
}

export interface VoiceSongsView {
  playId: string;
  voiceId: string;
  receipt: string | null;
  tracks: TrackView[];
}

export type VoiceSongAction =
  | { kind: 'cover'; index: number }
  | { kind: 'double_click'; index: number }
  | { kind: 'next_play'; index: number };

export interface VoiceSongsClient {
  playerVoiceSongs(epoch: string, playId: string): Promise<VoiceSongsView>;
  playerVoiceSongPlay(
    epoch: string,
    playId: string,
    receipt: string,
    action: VoiceSongAction,
    quality: Quality,
  ): Promise<PlayerView>;
}
