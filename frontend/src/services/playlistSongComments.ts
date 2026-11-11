import { failure, type AccountData } from './contracts.ts';

export type PlaylistSongCommentMenu = { kind: 'eligible'; receipt: string } | { kind: 'hidden' };

export interface PlaylistSongCommentClient {
  playlistSongRowCommentCount(epoch: string, receipt: string): Promise<AccountData<number | null>>;
}

export interface PlaylistSongCommentScope {
  epoch: string;
  menu: PlaylistSongCommentMenu;
  live: () => boolean;
}

const accountChanged = new Set(['stale', 'profile_changed', 'session_expired']);

export async function preparePlaylistSongComment(
  core: PlaylistSongCommentClient,
  scope: PlaylistSongCommentScope,
  signal: AbortSignal,
): Promise<{ count: number | null } | undefined> {
  const current = () => !signal.aborted && scope.live();
  if (scope.menu.kind !== 'eligible' || !current()) return;
  try {
    const { epoch, value } = await core.playlistSongRowCommentCount(
      scope.epoch,
      scope.menu.receipt,
    );
    if (!current() || epoch !== scope.epoch) return;
    return {
      count: typeof value === 'number' && Number.isSafeInteger(value) && value > 0 ? value : null,
    };
  } catch (cause) {
    if (!current() || accountChanged.has(failure(cause).code)) return;
    // Optional row receipts can expire after a failed refresh while the same
    // row still owns the menu. Other actions keep their own source checks.
    return { count: null };
  }
}
