import type { CoreClient } from './contracts.ts';
import {
  time,
  type PlaySelection,
  type PlayerOrigin,
  type PlayerView,
  type Quality,
} from './player.ts';
import type { SearchItem } from './search.ts';
import { albumCredits, catalogResourceSelection } from './catalogCards.ts';

export function searchCardKey(item: SearchItem): string {
  switch (item.kind) {
    case 'related':
      return `related:${item.keyword}`;
    case 'song':
      return `song:${item.track.id}`;
    case 'playlist':
      return `playlist:${item.playlist.id}`;
    case 'album':
      return `album:${item.album.id}`;
    case 'artist':
      return `artist:${item.artist.id}`;
    case 'podcast':
      return `podcast:${item.podcast.id}`;
    case 'voice':
      return `voice:${item.voice.id}`;
    case 'mv':
      return `mv:${item.mv.id}`;
    case 'user':
      return `user:${item.user.id}`;
  }
}
const positiveResourceId = (id: string | null | undefined): id is string =>
  !!id && /^\d*[1-9]\d*$/.test(id);

export function searchAlbumCredits(item: SearchItem) {
  if (item.kind !== 'album') return [];
  return albumCredits(item.album).flatMap(({ name, id }) => {
    const label = name.trim();
    return label ? [{ name: label, id: positiveResourceId(id) ? id : null }] : [];
  });
}
/** Only the native-verified creator may be used; playlist IDs are a separate namespace. */
export function searchPlaylistCreator(item: SearchItem) {
  if (item.kind !== 'playlist') return null;
  const { creatorId: id, creator } = item.playlist;
  const name = creator.trim();
  return name && positiveResourceId(id) ? { id, name } : null;
}
/** Only a native-projected MV identity can open the video player. */
export function searchMvPlaybackTarget(item: SearchItem): string | null {
  return item.kind === 'mv' && item.mv.kind === 'mv' ? item.mv.mvId : null;
}
const compactVideoCount = new Intl.NumberFormat('zh-CN', {
  notation: 'compact',
  maximumFractionDigits: 1,
});
const fullVideoCount = new Intl.NumberFormat('zh-CN');

/** Missing/zero values are not evidence of the video's duration or popularity. */
export function searchVideoCoverStats(item: SearchItem) {
  if (item.kind !== 'mv') return null;
  const { durationMs, playCount } = item.mv;
  const duration =
    Number.isSafeInteger(durationMs) && durationMs > 0 ? time(durationMs / 1000) : null;
  const count =
    Number.isSafeInteger(playCount) && playCount > 0 ? compactVideoCount.format(playCount) : null;
  if (!duration && !count) return null;
  return {
    duration,
    count,
    description: [
      count && `播放次数：${fullVideoCount.format(playCount)}`,
      duration && `时长：${duration}`,
    ]
      .filter(Boolean)
      .join('，'),
  };
}
export type SearchQueueCollection = Extract<SearchItem, { kind: 'playlist' | 'album' }>;
export interface SearchQueueRequest {
  item: SearchQueueCollection;
  anchor: HTMLElement | null;
}

/** Ordinary search playlist/album menus use existing native collection queue operations. */
export function searchCardCanQueue(item: SearchItem): item is SearchQueueCollection {
  return item.kind === 'playlist' || item.kind === 'album';
}
export function searchCardCanPlay(item: SearchItem): boolean {
  return (
    ['playlist', 'album', 'artist'].includes(item.kind) ||
    (item.kind === 'podcast' && !item.podcast.unavailable) ||
    (item.kind === 'voice' && !item.voice.unavailable)
  );
}
/** Search songs share catalog identity, never a backing media ID in another namespace. */
export function searchCurrentSongId(view?: Pick<PlayerView, 'resourceKind' | 'track'>) {
  return view?.resourceKind === 'catalog' ? view.track?.id : undefined;
}
export function searchCurrentVoiceId(view?: Pick<PlayerView, 'resourceKind' | 'track'>) {
  return view?.resourceKind === 'voice' ? (view.track?.voiceId ?? undefined) : undefined;
}
export function searchCardIsCurrent(
  item: SearchItem,
  origin?: PlayerOrigin | null,
  voiceId?: string,
): boolean {
  if (item.kind === 'voice') return item.voice.id === voiceId;
  if (!origin || !['playlist', 'album', 'artist', 'podcast'].includes(item.kind)) return false;
  return searchCardKey(item) === `${origin.kind}:${origin.id}`;
}
/** A search episode always resolves its own native details, regardless of parent radio. */
export function searchVoiceSelection(item: Extract<SearchItem, { kind: 'voice' }>) {
  return item.voice.unavailable
    ? undefined
    : { kind: 'search_result_voice' as const, id: item.voice.id };
}
/** Prepare from the existing account-bound catalog/radio snapshots, not search metadata. */
export async function searchCardSelection(
  core: Pick<CoreClient, 'catalogPage' | 'podcastPage'>,
  epoch: string,
  item: Exclude<SearchItem, { kind: 'voice' }>,
  quality: Quality,
): Promise<PlaySelection> {
  if (!searchCardCanPlay(item)) throw { code: 'unavailable', message: '此内容暂不可播放' };
  if (item.kind === 'playlist')
    return { kind: 'playlist', id: item.playlist.id, start_id: null, revision: null, quality };
  if (item.kind === 'artist' || item.kind === 'album') {
    return catalogResourceSelection(
      core,
      epoch,
      { kind: item.kind, id: item.kind === 'artist' ? item.artist.id : item.album.id },
      quality,
    );
  }
  if (item.kind === 'podcast') {
    const scope = { kind: 'radio' as const, id: item.podcast.id, ascending: true };
    const view = await core.podcastPage(
      epoch,
      { scope, keyword: '', offset: 0, limit: 100, revision: null },
      true,
    );
    if (view.epoch !== epoch) throw { code: 'stale', message: '账号已切换' };
    const id = view.episodes.find((episode) => !episode.unavailable)?.id;
    if (!id) throw { code: 'unavailable', message: '暂无可播放声音' };
    return { kind: 'podcast', scope, revision: view.revision, start_id: id };
  }
  throw { code: 'unavailable', message: '此内容暂不可播放' };
}
