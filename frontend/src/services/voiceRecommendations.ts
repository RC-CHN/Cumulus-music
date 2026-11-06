import type { VoicePlaybackScope } from './voiceDetail.ts';
export {
  sameVoicePlaybackScope as voiceRecommendationMatches,
  voicePlaybackScope as voiceRecommendationsSeed,
} from './voiceDetail.ts';

export type VoiceRecommendationsSeed = VoicePlaybackScope;

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
