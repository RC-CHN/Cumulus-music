import type { AccountData } from './contracts';

export interface PlayerPodcastResumeTarget {
  epoch: string;
  playId: string;
  entryId: string;
}

export interface PlayerPodcastResumeView {
  radioId: string;
  enabled: boolean;
  pending: boolean;
}

export type PlayerPodcastResumeAction =
  | { kind: 'view' }
  | { kind: 'change'; enabled: boolean }
  | { kind: 'reconcile' };

export interface PlayerPodcastResumeClient {
  playerPodcastResume(
    epoch: string,
    playId: string,
    entryId: string,
    action: PlayerPodcastResumeAction,
  ): Promise<AccountData<PlayerPodcastResumeView | null>>;
}
