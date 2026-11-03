import type { TrackView } from './contracts.ts';
import { searchLiteral } from './searchMatch.ts';

/** Match the visible song metadata without disturbing source collection positions. */
export function filterTrackScope(tracks: TrackView[], keyword: string) {
  const query = searchLiteral(keyword);
  if (!query) return { tracks, positions: null };
  const match = new RegExp(query, 'iu');
  const filtered: TrackView[] = [];
  const positions: number[] = [];
  tracks.forEach((track, index) => {
    if (
      !track.missing &&
      [track.name, ...track.artists, track.album].some((text) => match.test(text))
    ) {
      filtered.push(track);
      positions.push(index);
    }
  });
  return { tracks: filtered, positions };
}
