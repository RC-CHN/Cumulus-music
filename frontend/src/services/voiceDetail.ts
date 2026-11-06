import type { PlayerView } from './player.ts';

export interface VoiceDetail {
  id: string;
  creator: { id: string | null; name: string } | null;
  releaseTime: number | null;
  description?: string;
  coverUrl?: string;
  playCount?: number | string;
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
export interface VoicePlaybackScope {
  epoch: string;
  playId: string;
  voiceId: string;
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

// VoiceCover uses M229: numbers go through M32.f, text is displayed verbatim.
export function formatVoicePlayCount(value: number | string | null | undefined): string | null {
  if (typeof value === 'string') return value || null;
  if (typeof value !== 'number' || !Number.isFinite(value) || value <= 0) return null;
  if (value > 100_000_000) return `${Math.floor(value / 10_000_000) / 10}亿`;
  if (value > 100_000) return `${Math.floor(value / 1000) / 10}万`;
  return String(Math.floor(value));
}

export function playbackVoiceDetail(view: PlayerView | undefined) {
  const track = view?.track;
  if (!track || !(view?.resourceKind === 'voice' || track.voiceId)) return null;
  const voiceId = identifier(track.voiceId);
  const detail = voiceId && view.voiceDetail?.id === voiceId ? view.voiceDetail : null;
  return { id: voiceId, title: track.name, ...voiceDetailPresentation(detail, !!view.playId) };
}

export function voiceDetailPresentation(detail: VoiceDetail | null, active = true) {
  const creator = detail?.creator?.name
    ? { name: detail.creator.name, id: identifier(detail.creator.id) }
    : null;
  const radio = detail?.radio && identifier(detail.radio.id) ? detail.radio : undefined;
  return {
    creator,
    releaseDate: formatVoiceReleaseDate(detail?.releaseTime),
    ...(active && detail?.description ? { description: detail.description } : {}),
    ...(active && detail?.coverUrl ? { coverUrl: detail.coverUrl } : {}),
    ...(active && detail?.playCount !== undefined ? { playCount: detail.playCount } : {}),
    ...(active && radio ? { radio } : {}),
  };
}

export function voicePlaybackScope(
  view: PlayerView | undefined,
  epoch: string,
): VoicePlaybackScope | undefined {
  if (
    !view?.playId ||
    view.epoch !== epoch ||
    view.fm ||
    !['playing', 'paused', 'buffering'].includes(view.state)
  )
    return;
  const voiceId = identifier(view.track?.voiceId);
  if (!voiceId || view.voiceDetail?.id !== voiceId) return;
  return { epoch, playId: view.playId, voiceId };
}

export function sameVoicePlaybackScope(
  current: VoicePlaybackScope | undefined,
  target: VoicePlaybackScope,
) {
  return (
    !!current &&
    current.epoch === target.epoch &&
    current.playId === target.playId &&
    current.voiceId === target.voiceId
  );
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
