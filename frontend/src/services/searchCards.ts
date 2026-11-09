import type { ArtistReference, CoreClient } from './contracts.ts';
import {
  time,
  type PlaySelection,
  type PlayerOrigin,
  type PlayerView,
  type SearchPodcastRowGesture,
  type Quality,
} from './player.ts';
import type { SearchCategory, SearchItem, SearchSection } from './search.ts';
import { albumCredits, catalogResourceSelection } from './catalogCards.ts';
import { podcastProgramPublicLink, type PodcastProgramShare } from './podcast.ts';
import { searchLiteral } from './searchMatch.ts';

export interface SearchVoiceLinkRequest {
  item: Extract<SearchItem, { kind: 'voice' }>;
  action: 'share' | 'copy';
  anchor: HTMLElement | null;
}

export function searchVoiceLinkSection(category: SearchCategory, section: SearchSection) {
  return (
    section.category === 'voices' &&
    (category === 'voices' || (category === 'all' && section.id.startsWith('search_block_voice:')))
  );
}

export function searchVoiceProgram(
  category: SearchCategory,
  section: SearchSection,
  item: SearchItem,
): PodcastProgramShare | undefined {
  if (
    item.kind !== 'voice' ||
    !searchVoiceLinkSection(category, section) ||
    !section.items.includes(item) ||
    !podcastProgramPublicLink(item.voice.id)
  )
    return undefined;
  const { id, name, creator } = item.voice;
  return { id, name, creator };
}

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

function linkedCredits(artists: ArtistReference[]) {
  return artists.flatMap(({ name, id }) => {
    const label = name.trim();
    return label ? [{ name: label, id: positiveResourceId(id) ? id : null }] : [];
  });
}
export function searchAlbumCredits(item: SearchItem) {
  return item.kind === 'album' ? linkedCredits(albumCredits(item.album)) : [];
}
export function searchAlbumAlias(item: SearchItem, keyword: string) {
  if (item.kind !== 'album') return '';
  const names = [...item.album.translations, ...item.album.aliases]
    .map((name) => name.trim())
    .filter(Boolean);
  const literal = searchLiteral(keyword);
  const pattern = literal ? new RegExp(literal, 'iu') : undefined;
  return names.find((name) => pattern?.test(name)) || names[0] || '';
}
export function searchVideoCredits(item: SearchItem) {
  return item.kind === 'mv' && searchMvPlaybackTarget(item) ? linkedCredits(item.mv.artists) : [];
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
    (item.kind === 'playlist' && !item.knownEmpty) ||
    item.kind === 'album' ||
    item.kind === 'artist' ||
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
export interface SearchPendingVoiceToggle {
  voiceId: string;
  key: string;
  pause: boolean;
  disabled: boolean;
}

/** A search episode always resolves its own native details, regardless of parent radio. */
export function searchVoiceSelection(item: Extract<SearchItem, { kind: 'voice' }>) {
  return item.voice.unavailable
    ? undefined
    : { kind: 'search_result_voice' as const, id: item.voice.id };
}
/** Comprehensive podcast overlays can only use an admitted native result. */
export function searchPodcastCardSelection(item: Extract<SearchItem, { kind: 'podcast' }>) {
  return !item.table && !item.podcast.unavailable && item.playbackReceipt
    ? { kind: 'search_podcast_card' as const, id: item.podcast.id, receipt: item.playbackReceipt }
    : undefined;
}

/** The classified table has its own native response authority and gesture. */
export function searchPodcastRowSelection(
  item: Extract<SearchItem, { kind: 'podcast' }>,
  gesture: SearchPodcastRowGesture,
) {
  return item.table && !item.podcast.unavailable && item.playbackReceipt
    ? {
        kind: 'search_podcast_row' as const,
        id: item.podcast.id,
        receipt: item.playbackReceipt,
        gesture,
      }
    : undefined;
}

/** Artist covers resolve the signed cohort and track list entirely in Rust. */
export async function searchCardSelection(
  core: Pick<CoreClient, 'catalogPage'>,
  epoch: string,
  item: Exclude<SearchItem, { kind: 'voice' | 'podcast' }>,
  quality: Quality,
): Promise<PlaySelection> {
  if (!searchCardCanPlay(item)) throw { code: 'unavailable', message: '此内容暂不可播放' };
  if (item.kind === 'playlist')
    return { kind: 'playlist', id: item.playlist.id, start_id: null, revision: null, quality };
  if (item.kind === 'artist') return { kind: 'search_artist_card', id: item.artist.id, quality };
  if (item.kind === 'album')
    return catalogResourceSelection(core, epoch, { kind: 'album', id: item.album.id }, quality);
  throw { code: 'unavailable', message: '此内容暂不可播放' };
}
