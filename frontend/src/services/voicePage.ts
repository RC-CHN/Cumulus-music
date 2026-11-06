import type { PlayerView } from './player.ts';
import { voicePlaybackScope, type VoiceDetail, type VoicePlaybackScope } from './voiceDetail.ts';
import type { VoiceRecommendationsClient } from './voiceRecommendations.ts';
import type { VoiceSongsClient } from './voiceSongs.ts';

export interface VoicePageScope extends VoicePlaybackScope {
  entryId: string;
}

export interface VinylVoiceDetailView extends VoicePageScope {
  title?: string;
  detail: VoiceDetail;
}

export interface VoiceDetailClient {
  playerVinylVoiceDetail(epoch: string, playId: string): Promise<VinylVoiceDetailView>;
}

export type VoicePageClient = VoiceDetailClient &
  Pick<
    VoiceRecommendationsClient,
    'playerVinylVoiceRecommendations' | 'playerVinylVoiceRecommendationsNext'
  > &
  Pick<VoiceSongsClient, 'playerVoiceSongs'>;

export function voicePageScope(
  view: PlayerView | undefined,
  epoch: string,
): VoicePageScope | undefined {
  const seed = voicePlaybackScope(view, epoch);
  if (seed && view?.entryId) return { ...seed, entryId: view.entryId };
}
