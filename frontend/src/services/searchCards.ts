import type { CoreClient } from './contracts.ts';
import type { PlaySelection, PlayerOrigin, PlayerView, Quality } from './player.ts';
import type { SearchItem } from './search.ts';
import { catalogResourceSelection } from './catalogCards.ts';

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
    (item.kind === 'voice' && !item.voice.unavailable)
  );
}
/** Search songs share catalog identity, never a backing media ID in another namespace. */
export function searchCurrentSongId(view?: Pick<PlayerView, 'resourceKind' | 'track'>) {
  return view?.resourceKind === 'catalog' ? view.track?.id : undefined;
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
    return catalogResourceSelection(
      core,
      epoch,
      { kind: item.kind, id: item.kind === 'artist' ? item.artist.id : item.album.id },
      quality,
    );
  }
  if (item.kind === 'voice' && !item.voice.radioId) return { kind: 'voice', id: item.voice.id };
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
    if (item.kind === 'voice') {
      const episode = view.episodes.find((candidate) => candidate.id === item.voice.id);
      // The radio page may omit this episode (for example, beyond its first 100).
      // Let native resolve exactly the clicked episode; never substitute its neighbour.
      if (!episode) return { kind: 'voice', id: item.voice.id };
      if (episode.unavailable) throw { code: 'unavailable', message: '此内容暂不可播放' };
    }
    const id =
      item.kind === 'voice' ? item.voice.id : view.episodes.find((e) => !e.unavailable)?.id;
    if (!id) throw { code: 'unavailable', message: '暂无可播放声音' };
    return { kind: 'podcast', scope, revision: view.revision, start_id: id };
  }
  throw { code: 'unavailable', message: '此内容暂不可播放' };
}
