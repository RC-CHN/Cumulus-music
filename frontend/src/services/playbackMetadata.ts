import type { CatalogResource } from './catalog.ts';
import type { TrackView } from './contracts.ts';

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
