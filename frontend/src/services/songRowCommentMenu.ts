import { failure, type AccountData } from './contracts.ts';

export type SongRowCommentMenu = { kind: 'eligible'; receipt: string } | { kind: 'hidden' };

export interface SongRowCommentClient {
  playlistSongRowCommentCount(epoch: string, receipt: string): Promise<AccountData<number | null>>;
  searchSongRowCommentCount(epoch: string, receipt: string): Promise<AccountData<number | null>>;
}

export interface SongRowCommentScope {
  source: 'playlist' | 'search';
  epoch: string;
  menu?: SongRowCommentMenu;
  live: () => boolean;
}

const accountChanged = new Set(['stale', 'profile_changed', 'session_expired']);

export async function prepareSongRowComment(
  core: SongRowCommentClient,
  scope: SongRowCommentScope,
  signal: AbortSignal,
): Promise<{ count: number | null } | undefined> {
  const current = () => !signal.aborted && scope.live();
  if (scope.menu?.kind !== 'eligible' || !current()) return;
  const {
    epoch: owner,
    menu: { receipt },
  } = scope;
  try {
    const { epoch, value } = await (scope.source === 'search'
      ? core.searchSongRowCommentCount(owner, receipt)
      : core.playlistSongRowCommentCount(owner, receipt));
    if (!current() || epoch !== owner) return;
    return {
      count: typeof value === 'number' && Number.isSafeInteger(value) && value > 0 ? value : null,
    };
  } catch (cause) {
    const { code } = failure(cause);
    if (!current() || accountChanged.has(code)) return;
    // An optional count receipt can expire while its exact renderer row
    // remains current. Keep the other menu actions and their own fences.
    return { count: null };
  }
}
