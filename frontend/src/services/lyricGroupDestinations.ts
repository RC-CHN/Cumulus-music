import type { TrackView } from './contracts.ts';
import type { SearchClient, SearchGroupDownloadTarget } from './search.ts';

export type LyricGroupCollectionPlan = {
  receipt: string | null;
  canCreate: boolean;
  selectedCount: number;
  songIds: string[];
};

export function lyricCollectionTracks(
  plan: LyricGroupCollectionPlan,
  selected: readonly TrackView[],
): TrackView[] | undefined {
  if (plan.selectedCount !== selected.length || !!plan.receipt !== !!plan.songIds.length) return;
  const byId = new Map(selected.map((track, index) => [track.id, { track, index }]));
  if (byId.size !== selected.length || new Set(plan.songIds).size !== plan.songIds.length) return;
  const tracks: TrackView[] = [];
  let previous = -1;
  for (const id of plan.songIds) {
    const entry = byId.get(id);
    if (!entry || entry.index <= previous) return;
    tracks.push(entry.track);
    previous = entry.index;
  }
  return tracks;
}

// Optional, read-only menu metadata. Failure or a slow local check hides only
// Download; a retired row/visit must not open the old group menu at all.
export async function prepareLyricDownloadVisibility(
  client: Pick<SearchClient, 'searchGroupDownload'>,
  epoch: string,
  anchor: SearchGroupDownloadTarget,
  live: () => boolean,
  signal: AbortSignal,
  timeoutMs = 1200,
): Promise<boolean | undefined> {
  const current = () => !signal.aborted && live();
  if (!current()) return;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let abandon: (() => void) | undefined;
  try {
    const deadline = new Promise<boolean | undefined>((resolve) => {
      timer = setTimeout(() => resolve(false), timeoutMs);
      abandon = () => resolve(undefined);
      signal.addEventListener('abort', abandon, { once: true });
    });
    const visible = await Promise.race([
      Promise.resolve()
        .then(() =>
          current()
            ? client.searchGroupDownload(epoch, { kind: 'preview_lyrics_anchor', anchor })
            : undefined,
        )
        .then(
          (reply) => reply?.kind === 'lyric_menu' && reply.epoch === epoch && reply.show === true,
          () => false,
        ),
      deadline,
    ]);
    if (current()) return visible;
  } finally {
    clearTimeout(timer);
    if (abandon) signal.removeEventListener('abort', abandon);
  }
}
