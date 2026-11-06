import type { PlayerView } from './player.ts';

export interface VoiceDetail {
  id: string;
  creator: { id: string | null; name: string } | null;
  releaseTime: number | null;
  description?: string;
  radio?: VoicePlaybackPodcast;
}
export interface VoicePlaybackPodcast {
  id: string;
  name: string;
  coverUrl?: string;
  subscriberCount?: number;
  ownedByViewer: boolean;
}
export interface VoicePodcastTarget {
  epoch: string;
  playId: string;
  voiceId: string;
  radioId: string;
}
export interface VoiceCreatorTarget {
  epoch: string;
  playId: string;
  voiceId: string;
  userId: string;
}
const identifier = (value: string | null | undefined) =>
  typeof value === 'string' && /^[1-9][0-9]*$/.test(value) ? value : null;

// Official M/103.p uses dayjs(milliseconds).format('YYYY-MM-DD') in local time.
// Missing metadata is kept unknown, rather than passing undefined to dayjs and
// accidentally showing today's date.
export function formatVoiceReleaseDate(value: number | null | undefined) {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value <= 0 || value > 8.64e15)
    return null;
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return null;
  return `${String(date.getFullYear()).padStart(4, '0')}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

export function playbackVoiceDetail(view: PlayerView | undefined) {
  const track = view?.track;
  if (!track || !(view?.resourceKind === 'voice' || track.voiceId)) return null;
  const voiceId = identifier(track.voiceId);
  const detail = voiceId && view.voiceDetail?.id === voiceId ? view.voiceDetail : null;
  const creator = detail?.creator?.name
    ? { name: detail.creator.name, id: identifier(detail.creator.id) }
    : null;
  const radio = detail?.radio && identifier(detail.radio.id) ? detail.radio : undefined;
  return {
    id: voiceId,
    title: track.name,
    creator,
    releaseDate: formatVoiceReleaseDate(detail?.releaseTime),
    ...(view.playId && detail?.description ? { description: detail.description } : {}),
    ...(view.playId && radio ? { radio } : {}),
  };
}

export function voicePodcastTarget(view: PlayerView | undefined): VoicePodcastTarget | null {
  const detail = playbackVoiceDetail(view);
  if (!view?.playId || !detail?.id || !detail.radio) return null;
  return { epoch: view.epoch, playId: view.playId, voiceId: detail.id, radioId: detail.radio.id };
}

export function voicePodcastTargetIsCurrent(
  target: VoicePodcastTarget,
  view: PlayerView | undefined,
) {
  const current = voicePodcastTarget(view);
  return (
    !!current &&
    current.epoch === target.epoch &&
    current.playId === target.playId &&
    current.voiceId === target.voiceId &&
    current.radioId === target.radioId
  );
}

export function voiceCreatorTarget(view: PlayerView | undefined): VoiceCreatorTarget | null {
  const detail = playbackVoiceDetail(view);
  if (!view?.playId || !detail?.id || !detail.creator?.id) return null;
  return { epoch: view.epoch, playId: view.playId, voiceId: detail.id, userId: detail.creator.id };
}

export function voiceCreatorTargetIsCurrent(
  target: VoiceCreatorTarget,
  view: PlayerView | undefined,
) {
  const current = voiceCreatorTarget(view);
  return (
    !!current &&
    current.epoch === target.epoch &&
    current.playId === target.playId &&
    current.voiceId === target.voiceId &&
    current.userId === target.userId
  );
}
