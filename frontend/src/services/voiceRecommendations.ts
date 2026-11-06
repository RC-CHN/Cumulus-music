import type { PlayerView } from './player.ts';
import { playbackVoiceDetail } from './voiceDetail.ts';

export interface VoiceRecommendationsSeed {
  epoch: string;
  playId: string;
  voiceId: string;
}

export interface VoiceRecommendationCard {
  radioId: string;
  name: string;
  coverUrl?: string;
  playCount: number | string;
}

export interface VoiceRecommendationsPage {
  playId: string;
  voiceId: string;
  receipt: string;
  page: number;
  pageCount: number;
  cards: VoiceRecommendationCard[];
}

export interface VoiceRecommendationTarget extends VoiceRecommendationsSeed {
  receipt: string;
  page: number;
  radioId: string;
}

export interface VoiceRecommendationsClient {
  playerVinylVoiceRecommendations(epoch: string, playId: string): Promise<VoiceRecommendationsPage>;
  playerVinylVoiceRecommendationsNext(
    epoch: string,
    playId: string,
    receipt: string,
  ): Promise<VoiceRecommendationsPage>;
  playerVinylVoiceRecommendationTarget(target: VoiceRecommendationTarget): Promise<string>;
}

export function voiceRecommendationsSeed(
  view: PlayerView | undefined,
  epoch: string,
): VoiceRecommendationsSeed | undefined {
  if (
    !view?.playId ||
    view.epoch !== epoch ||
    view.fm ||
    !['playing', 'paused', 'buffering'].includes(view.state)
  )
    return;
  const voiceId = playbackVoiceDetail(view)?.id;
  if (!voiceId || view.voiceDetail?.id !== voiceId) return;
  return { epoch, playId: view.playId, voiceId };
}

export function voiceRecommendationMatches(
  seed: VoiceRecommendationsSeed | undefined,
  target: VoiceRecommendationsSeed,
) {
  return (
    !!seed &&
    seed.epoch === target.epoch &&
    seed.playId === target.playId &&
    seed.voiceId === target.voiceId
  );
}
