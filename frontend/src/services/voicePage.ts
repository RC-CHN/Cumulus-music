import type { AccountData } from './contracts.ts';
import type { PlayerView } from './player.ts';
import type { PodcastClient, PodcastMutation, PodcastSearchFollow } from './podcast.ts';
import {
  sameVoicePlaybackScope,
  voiceCreatorTarget,
  voiceDetailPresentation,
  voicePlaybackScope,
  voicePodcastTarget,
  type VoiceCreatorTarget,
  type VoiceDetail,
  type VoicePodcastTarget,
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

export type VoicePageCreatorTarget = VoiceCreatorTarget & { receipt?: string };
export type VoicePagePodcastTarget = VoicePodcastTarget & { receipt?: string };

export function sameVoicePageTarget(
  current: (VoicePlaybackScope & { receipt?: string }) | undefined,
  target: VoicePlaybackScope & { receipt?: string },
) {
  return sameVoicePlaybackScope(current, target) && current?.receipt === target.receipt;
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

export function voicePageSubscriptionAction(
  core: VoiceDetailActionsClient & Pick<PodcastClient, 'podcastCommand'>,
  target: VoicePagePodcastTarget,
  current: PodcastSearchFollow,
): () => Promise<AccountData<PodcastMutation>> {
  const { epoch, playId, radioId, receipt } = target;
  if (receipt) {
    const scope = { epoch, playId, receipt };
    return current.kind === 'toggle'
      ? () => core.changeFreshVinylPodcastSubscription(scope, current.subscribed)
      : () => core.reconcileFreshVinylPodcastSubscription(scope);
  }
  return current.kind === 'toggle'
    ? () =>
        core.podcastCommand(epoch, {
          kind: 'change_vinyl_subscription',
          play_id: playId,
          expected: current.subscribed,
        })
    : () => core.podcastCommand(epoch, { kind: 'reconcile', action: 'subscribe', id: radioId });
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
): VoicePageCreatorTarget | undefined {
  const current = currentVoicePageDetail(detail, view, epoch);
  if (!current) return;
  const userId = voiceDetailPresentation(current.detail).creator?.id;
  if (!userId) return;
  if (current.actionReceipt)
    return {
      epoch,
      playId: current.playId,
      voiceId: current.voiceId,
      receipt: current.actionReceipt,
      userId,
    };
  const target = voiceCreatorTarget(view);
  if (target?.userId === userId) return target;
}

export function voicePagePodcastTarget(
  detail: VinylVoiceDetailView | undefined,
  view: PlayerView | undefined,
  epoch: string,
): VoicePagePodcastTarget | undefined {
  const current = currentVoicePageDetail(detail, view, epoch);
  if (!current) return;
  const radioId = voiceDetailPresentation(current.detail).radio?.id;
  if (!radioId) return;
  if (current.actionReceipt)
    return {
      epoch,
      playId: current.playId,
      voiceId: current.voiceId,
      receipt: current.actionReceipt,
      radioId,
    };
  const target = voicePodcastTarget(view);
  if (target?.radioId === radioId) return target;
}
