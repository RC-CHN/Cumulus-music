import type { CoreClient } from './contracts.ts';
import type { PlaySelection, PlayerOrigin, Quality } from './player.ts';
import type { SearchItem } from './search.ts';

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
export function searchCardCanPlay(item: SearchItem): boolean {
  return (
    ['playlist', 'album', 'artist'].includes(item.kind) ||
    (item.kind === 'podcast' && !item.podcast.unavailable) ||
    (item.kind === 'voice' && !!item.voice.radioId && !item.voice.unavailable)
  );
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
/** Prepare from the existing account-bound catalog/radio snapshots, not search metadata. */
export async function searchCardSelection(
  core: Pick<CoreClient, 'catalogPage' | 'podcastPage'>,
  epoch: string,
  item: SearchItem,
  quality: Quality,
): Promise<PlaySelection> {
  if (!searchCardCanPlay(item)) throw { code: 'unavailable', message: '此内容暂不可播放' };
  if (item.kind === 'playlist')
    return { kind: 'playlist', id: item.playlist.id, start_id: null, revision: null, quality };
  if (item.kind === 'artist' || item.kind === 'album') {
    const query = { kind: item.kind, id: item.kind === 'artist' ? item.artist.id : item.album.id };
    const result = await core.catalogPage(epoch, query, true);
    if (result.epoch !== epoch) throw { code: 'stale', message: '账号已切换' };
    const view = result.value;
    if (!view || !['artist', 'album'].includes(view.page.kind))
      throw { code: 'unavailable', message: '暂时无法读取歌曲，请重试' };
    const songs = view.page.kind === 'artist' || view.page.kind === 'album' ? view.page.tracks : [];
    const index = songs.findIndex((t) => !t.missing);
    if (index < 0) throw { code: 'unavailable', message: '暂无可播放歌曲' };
    return {
      kind: 'catalog',
      pages: [{ query: view.query, revision: view.revision }],
      index,
      quality,
    };
  }
  if (item.kind === 'podcast' || item.kind === 'voice') {
    const scope = {
      kind: 'radio' as const,
      id: item.kind === 'podcast' ? item.podcast.id : item.voice.radioId!,
      ascending: true,
    };
    const view = await core.podcastPage(
      epoch,
      { scope, keyword: '', offset: 0, limit: 100, revision: null },
      true,
    );
    if (view.epoch !== epoch) throw { code: 'stale', message: '账号已切换' };
    const id =
      item.kind === 'voice' ? item.voice.id : view.episodes.find((e) => !e.unavailable)?.id;
    if (!id) throw { code: 'unavailable', message: '暂无可播放声音' };
    return { kind: 'podcast', scope, revision: view.revision, start_id: id };
  }
  throw { code: 'unavailable', message: '此内容暂不可播放' };
}
