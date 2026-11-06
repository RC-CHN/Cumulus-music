import type { AccountData } from './contracts.ts';
import type { PlayerView } from './player.ts';
import type { PodcastMutation } from './podcast.ts';
import {
  sameVoicePlaybackScope,
  voiceCreatorTarget,
  voicePlaybackScope,
  voicePodcastTarget,
  type VoiceDetail,
  type VoicePlaybackScope,
} from './voiceDetail.ts';
import type { VoiceRecommendationsClient } from './voiceRecommendations.ts';
import type { VoiceSongsClient } from './voiceSongs.ts';

export interface VoicePageScope extends VoicePlaybackScope {
  entryId: string;
}

export interface VinylVoiceDetailView extends VoicePageScope {
  title?: string;
  detail: VoiceDetail;
  actionReceipt?: string;
}

export interface VoiceDetailClient {
  playerVinylVoiceDetail(epoch: string, playId: string): Promise<VinylVoiceDetailView>;
}

export interface VoiceDetailActionScope {
  epoch: string;
  playId: string;
  receipt: string;
}

export interface VoiceDetailRouteTarget extends VoiceDetailActionScope {
  kind: 'creator' | 'radio';
}

export interface VoiceDetailActionsClient {
  playerVinylVoiceDetailTarget(target: VoiceDetailRouteTarget): Promise<string>;
  changeFreshVinylPodcastSubscription(
    scope: VoiceDetailActionScope,
    expected: boolean,
  ): Promise<AccountData<PodcastMutation>>;
  reconcileFreshVinylPodcastSubscription(
    scope: VoiceDetailActionScope,
  ): Promise<AccountData<PodcastMutation>>;
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

export function currentVoicePageDetail(
  detail: VinylVoiceDetailView | undefined,
  view: PlayerView | undefined,
  epoch: string,
) {
  const scope = voicePageScope(view, epoch);
  if (
    detail &&
    scope?.entryId === detail.entryId &&
    detail.detail.id === detail.voiceId &&
    sameVoicePlaybackScope(scope, detail)
  )
    return detail;
}

export function voicePageCreatorTarget(
  detail: VinylVoiceDetailView | undefined,
  view: PlayerView | undefined,
  epoch: string,
) {
  const current = currentVoicePageDetail(detail, view, epoch);
  const target = voiceCreatorTarget(view);
  if (target && current?.detail.creator?.id === target.userId) return target;
}

export function voicePagePodcastTarget(
  detail: VinylVoiceDetailView | undefined,
  view: PlayerView | undefined,
  epoch: string,
) {
  const current = currentVoicePageDetail(detail, view, epoch);
  const target = voicePodcastTarget(view);
  if (target && current?.detail.radio?.id === target.radioId) return target;
}
