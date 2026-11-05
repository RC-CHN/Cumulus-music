import type { LyricsView, PlayerView } from './player.ts';

export interface LyricContributor {
  nickname: string;
  userId: string | null;
}

const kinds = [
  { kind: 'lyricContributor', label: '滚动歌词贡献者：' },
  { kind: 'translationContributor', label: '翻译贡献者：' },
] as const;
type LyricContributorKind = (typeof kinds)[number]['kind'];
type LyricCreditDocument = Pick<LyricsView, 'epoch' | 'playId' | LyricContributorKind>;
type PlaybackIdentity = Pick<PlayerView, 'epoch' | 'playId'>;

export interface LyricContributorTarget {
  epoch: string;
  playId: string;
  kind: LyricContributorKind;
  userId: string;
}

function contributorId(value: string | null | undefined): value is string {
  return typeof value === 'string' && /^[0-9]{1,128}$/.test(value) && /[1-9]/.test(value);
}

export function lyricContributorRows(
  document: LyricCreditDocument | undefined,
  playback: PlaybackIdentity | undefined,
) {
  if (
    !document ||
    !playback ||
    document.epoch !== playback.epoch ||
    document.playId !== playback.playId
  )
    return [];
  return kinds.flatMap(({ kind, label }) => {
    const credit = document[kind];
    if (!credit?.nickname) return [];
    const target: LyricContributorTarget | undefined = contributorId(credit.userId)
      ? { epoch: document.epoch, playId: document.playId, kind, userId: credit.userId }
      : undefined;
    return [{ kind, label, nickname: credit.nickname, target }];
  });
}

function matches(target: LyricContributorTarget, document: LyricCreditDocument | undefined) {
  return (
    !!document &&
    target.epoch === document.epoch &&
    target.playId === document.playId &&
    contributorId(target.userId) &&
    target.userId === document[target.kind]?.userId
  );
}

export async function confirmLyricContributorTarget(
  target: LyricContributorTarget,
  activeDocument: () => LyricCreditDocument | undefined,
  syncPlayback: () => Promise<PlaybackIdentity | undefined>,
  readCachedDocument: () => Promise<LyricCreditDocument>,
) {
  if (!matches(target, activeDocument())) return false;
  const playback = await syncPlayback();
  if (
    playback?.epoch !== target.epoch ||
    playback.playId !== target.playId ||
    !matches(target, activeDocument())
  )
    return false;
  // The renderer can still display an old copy while a same-play refresh
  // commits. Read the native document without triggering a new lyric request.
  const current = await readCachedDocument();
  return matches(target, current) && matches(target, activeDocument());
}
