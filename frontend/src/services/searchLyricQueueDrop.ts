import type { TrackView } from './contracts.ts';
import type { SearchLyricQueueDropTickets } from './player.ts';

type LyricDropHit = {
  track: Pick<TrackView, 'id' | 'local' | 'missing' | 'voiceId'>;
  lyricQueueDrop?: { receipt: string; right: 'playable' | 'copyright_blocked' };
};

function observed(hit: LyricDropHit | undefined) {
  const grant = hit?.lyricQueueDrop;
  if (
    hit?.track.id &&
    !hit.track.local &&
    !hit.track.missing &&
    !hit.track.voiceId &&
    grant?.receipt &&
    (grant.right === 'playable' || grant.right === 'copyright_blocked')
  )
    return grant;
}

/** M225 keeps first displayed representatives in selection order. The grabbed
 * hit remains independent, including when it is a later duplicate. */
export function lyricQueueDropTickets(
  displayed: readonly LyricDropHit[],
  grabbed: number,
  selected: ReadonlySet<string>,
): SearchLyricQueueDropTickets | undefined {
  const exact = displayed[grabbed];
  const anchor = observed(exact);
  if (!exact || !anchor) return;
  if (selected.size < 2)
    return anchor.right === 'playable'
      ? { anchor: anchor.receipt, rows: [anchor.receipt] }
      : undefined;
  if (!selected.has(exact.track.id)) return;
  const first = new Map<string, LyricDropHit>();
  for (const hit of displayed)
    if (selected.has(hit.track.id) && !first.has(hit.track.id)) first.set(hit.track.id, hit);
  const firstAnchor = observed(first.get(exact.track.id));
  if (!firstAnchor || firstAnchor.right !== anchor.right) return;
  const rows: string[] = [];
  let playable = false;
  for (const id of selected) {
    const row = observed(first.get(id));
    if (!row) return;
    playable ||= row.right === 'playable';
    rows.push(row.receipt);
  }
  if (
    playable &&
    new Set(rows).size === rows.length &&
    (anchor.receipt === firstAnchor.receipt || !rows.includes(anchor.receipt))
  )
    return { anchor: anchor.receipt, rows };
}
