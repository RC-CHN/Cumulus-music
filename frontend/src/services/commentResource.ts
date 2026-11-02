import type { AccountData, ArtistReference, TrackView } from './contracts';
import type { CommentResource } from './comments';
import type { EpisodeView } from './podcast';
import type { PlayerView, PlaySelection } from './player';

export type CommentResourceCard =
  | { kind: 'track'; track: TrackView }
  | { kind: 'voice'; episode: EpisodeView }
  | { kind: 'mv'; id: string; title: string; coverUrl: string | null; artists: ArtistReference[] };
export interface CommentResourceView {
  resource: CommentResource;
  card: CommentResourceCard | null;
  cached: boolean;
}
export interface CommentResourceClient {
  commentsResource(
    epoch: string,
    resource: CommentResource,
    refresh: boolean,
  ): Promise<AccountData<CommentResourceView>>;
}
export type CommentResourceDestination = { kind: 'album' | 'artist' | 'radio' | 'mv'; id: string };
export function commentResourceId(card: CommentResourceCard): string {
  return card.kind === 'track' ? card.track.id : card.kind === 'voice' ? card.episode.id : card.id;
}
export function currentCommentResource(
  card: CommentResourceCard,
  view: PlayerView | null | undefined,
): boolean {
  if (!view?.playId || !['playing', 'paused', 'buffering'].includes(view.state)) return false;
  return card.kind === 'track'
    ? view.resourceKind === 'catalog' && view.track?.id === card.track.id
    : card.kind === 'voice' &&
        view.resourceKind === 'voice' &&
        view.track?.voiceId === card.episode.id;
}
export function commentResourceSelection(
  card: CommentResourceCard,
  view?: PlayerView | null,
): PlaySelection | undefined {
  if (card.kind === 'track')
    return {
      kind: 'songs',
      ids: [card.track.id],
      index: 0,
      quality: view?.qualityPreferences.quality ?? 'standard',
    };
  if (card.kind === 'voice') return { kind: 'voice', id: card.episode.id };
}
