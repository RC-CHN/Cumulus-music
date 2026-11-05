import type { CatalogResource } from './catalog.ts';
import type { TrackView } from './contracts.ts';
import type { PlayerOrigin, PlayerView } from './player.ts';

type MetadataTrack = Pick<TrackView, 'artists' | 'artistRefs' | 'album' | 'albumId' | 'local'>;

export function playbackMetadata(track: MetadataTrack | null | undefined) {
  function link(kind: CatalogResource['kind'], id?: string | null): CatalogResource | null {
    return !track?.local && id && /^[1-9][0-9]*$/.test(id) ? { kind, id } : null;
  }
  const artists = (
    track?.artistRefs?.length
      ? track.artistRefs
      : (track?.artists ?? []).map((name) => ({ id: null, name }))
  )
    .filter((artist) => artist.name.trim())
    .map((artist) => ({ name: artist.name, target: link('artist', artist.id) }));
  const album = track?.album.trim();
  return {
    artists: artists.length ? artists : [{ name: '未知歌手', target: null }],
    album: { name: album || '未知', target: album ? link('album', track?.albumId) : null },
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
  const { origin, track } = view;
  if (origin && /^[0-9]*[1-9][0-9]*$/.test(origin.id)) {
    const { kind, id } = origin;
    const name =
      kind === 'album'
        ? (track.albumId === id && track.album.trim()) || '专辑'
        : kind === 'artist'
          ? track.artistRefs?.find((artist) => artist.id === id)?.name.trim() || '歌手'
          : kind === 'playlist'
            ? '歌单'
            : '播客';
    return { name, target: origin };
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
