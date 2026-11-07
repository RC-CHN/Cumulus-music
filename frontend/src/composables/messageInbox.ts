import type { MessageRow } from '../services/messages';

const key = (row: MessageRow) => (row.peerId ? `peer:${row.peerId}` : `row:${row.id}`);

// M/2092 places the new first page before the cached tail. An unchanged peer
// and last-message time preserve its locally read preview, but destructive
// capabilities on newly observed rows always come from the native reply.
export function mergePrivateInbox(
  previous: readonly MessageRow[],
  fresh: readonly MessageRow[],
  nextCursor: number | null,
) {
  const cached = new Map(previous.map((row) => [key(row), row]));
  const seen = new Set<string>();
  const rows: MessageRow[] = [];
  for (const row of [...fresh, ...previous]) {
    const id = key(row);
    if (seen.has(id)) continue;
    seen.add(id);
    const old = cached.get(id);
    if (old && row.peerId && row.time !== null && old.time === row.time) {
      rows.push(
        old.deletionReceipt === row.deletionReceipt && old.deletionPending === row.deletionPending
          ? old
          : { ...old, deletionReceipt: row.deletionReceipt, deletionPending: row.deletionPending },
      );
    } else rows.push(row);
  }
  return { rows, cursor: nextCursor === null ? null : (previous.at(-1)?.time ?? nextCursor) };
}
