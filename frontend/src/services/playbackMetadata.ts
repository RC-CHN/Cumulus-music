import type { CatalogResource } from './catalog.ts';
import type { TrackView } from './contracts.ts';
import type { PlayerOrigin, PlayerView } from './player.ts';

type MetadataTrack = Pick<TrackView, 'artists' | 'artistRefs' | 'album' | 'albumId' | 'local'>;

function artistCredits(
  track: Pick<TrackView, 'artists' | 'artistRefs'> | null | undefined,
  link: (id: string) => boolean,
) {
  const credits = (
    track?.artistRefs?.length
      ? track.artistRefs
      : (track?.artists ?? []).map((name) => ({ id: null, name }))
  )
    .filter((artist) => artist.name.trim())
    .map(({ name, id }) => ({
      name,
      target: id && link(id) ? { kind: 'artist' as const, id } : null,
    }));
  return credits.length ? credits : [{ name: '未知歌手', target: null }];
}

export function queueArtistCredits(track: TrackView, fm: boolean) {
  return artistCredits(
    track,
    (id) => !fm && !track.local && !track.voiceId && /^[0-9]*[1-9][0-9]*$/.test(id),
  );
}

export function playbackMetadata(track: MetadataTrack | null | undefined) {
  const link = (id: string) => !track?.local && /^[1-9][0-9]*$/.test(id);
  const album = track?.album.trim();
  return {
    artists: artistCredits(track, link),
    album: {
      name: album || '未知',
      target:
        album && track?.albumId && link(track.albumId)
          ? ({ kind: 'album', id: track.albumId } satisfies CatalogResource)
          : null,
    },
  };
}

export type PlaybackSourceDestination = PlayerOrigin | { kind: 'local' | 'cloud' | 'downloads' };

export function playbackSource(view: PlayerView | undefined): {
  name: string;
  target?: PlaybackSourceDestination;
} | null {
  // Official VoiceTitle omits provenance for every voice namespace, including
  // downloaded voices. Keep native origins intact for non-display consumers.
  if (!view?.track || view.fm || view.resourceKind === 'voice' || view.track.voiceId) return null;
  if (view.vinylSimilarOrigin && view.resourceKind === 'catalog') return { name: '相似歌曲' };
  const { origin, originName } = view;
  if (origin && /^[0-9]*[1-9][0-9]*$/.test(origin.id)) {
    if (origin.kind === 'podcast') return { name: '播客', target: origin };
    if (view.resourceKind === 'catalog') {
      return originName?.trim() ? { name: originName, target: origin } : { name: '未知' };
    }
  }
  if (view.searchOrigin && view.resourceKind === 'catalog') return { name: '搜索' };
  switch (view.resourceKind) {
    case 'local':
      return { name: '本地音乐', target: { kind: 'local' } };
    case 'cloud':
      return { name: '音乐云盘', target: { kind: 'cloud' } };
    case 'downloaded':
      return { name: '下载管理', target: { kind: 'downloads' } };
    default:
      return { name: '未知' };
  }
}
