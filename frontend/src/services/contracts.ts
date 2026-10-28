import type { PlayerClient } from './player';
import type { PlaylistClient } from './playlists';
import type { PlaylistCoverClient } from './playlistCover';
import type { DownloadClient } from './downloads';
import type { CloudClient } from './cloud';
import type { MvClient } from './mv';
import type { CommentClient } from './comments';
import type { CommentWriteClient } from './commentWrites';
import type { CommentFloorClient } from './commentFloor';
import type { LocalClient } from './local';
import type { LocalMatchClient } from './localMatching';
import type { PodcastClient } from './podcast';
import type { TogetherClient } from './together';
import type { AuthClient } from './auth';
import type { RankingClient } from './rankings';
import type { FmClient } from './fm';
import type { SceneFmClient } from './sceneFm';
import type { MemberClient } from './member';
import type { CatalogClient } from './catalog';
/** Wire format returned by core::AppInfo. Keep the IPC test in sync. */
export interface AppInfo {
  name: string;
  coreVersion: string;
  protocolVersion: number;
}

/** UI-facing use cases; no Tauri types or raw HTTP API in this interface. */
export interface CoreClient
  extends PlayerClient,
    PlaylistClient,
    PlaylistCoverClient,
    DownloadClient,
    CloudClient,
    MvClient,
    CommentClient,
    CommentWriteClient,
    CommentFloorClient,
    LocalClient,
    LocalMatchClient,
    PodcastClient,
    TogetherClient,
    AuthClient,
    RankingClient,
    FmClient,
    SceneFmClient,
    MemberClient,
    CatalogClient {
  getAppInfo(): Promise<AppInfo>;
  accountSnapshot(): Promise<AccountView>;
  restoreAccount(): Promise<AccountView>;
  beginQr(): Promise<QrView>;
  pollQr(operationId: string): Promise<QrStatus>;
  cancelQr(operationId: string): Promise<void>;
  playlists(): Promise<AccountData<PlaylistView[]>>;
  playlistPage(id: string, page: PageRequest): Promise<AccountData<TrackPage>>;
  searchSongs(keyword: string, page: PageRequest): Promise<AccountData<TrackPage>>;
  dailySongs(): Promise<AccountData<TrackPage>>;
  likedIds(): Promise<AccountData<string[]>>;
  setLiked(epoch: string, id: string, liked: boolean): Promise<AccountData<LikeResult>>;
}

export interface AccountView {
  epoch: string;
  status: 'unknown' | 'authenticated' | 'expired' | 'signed_out';
  nickname: string | null;
  avatarUrl: string | null;
  /** Additive native capability; absent on older core versions. */
  canSignOut?: boolean;
  loginNotice?: {
    kind: 'expired' | 'reauthenticate';
    title: string | null;
    message: string;
  } | null;
}
export interface AccountData<T> {
  epoch: string;
  value: T;
}
export interface PageRequest {
  offset: number;
  limit: number;
  revision?: string | null;
}
export interface PlaylistView {
  id: string;
  name: string;
  coverUrl: string | null;
  description: string;
  creator: string;
  trackCount: number;
  owned: boolean;
  favorite: boolean;
  private: boolean;
}
export interface ArtistReference {
  id: string | null;
  name: string;
}
export interface TrackView {
  voiceId?: string;
  id: string;
  name: string;
  artists: string[];
  artistRefs?: ArtistReference[];
  album: string;
  albumId?: string | null;
  coverUrl: string | null;
  durationMs: number;
  fee: number;
  missing: boolean;
  local: boolean;
}
export interface TrackPage {
  revision: string | null;
  tracks: TrackView[];
  total: number;
  offset: number;
  more: boolean;
  playlist: PlaylistView | null;
}
export type QrStatus =
  | 'idle'
  | 'creating'
  | 'waiting_scan'
  | 'waiting_confirmation'
  | 'authorized'
  | 'expired'
  | 'challenge'
  | 'cancelled'
  | 'error';
export interface QrView {
  operationId: string;
  status: QrStatus;
  modules: boolean[][];
}
export interface LikeResult {
  id: string;
  phase: 'sending' | 'verifying' | 'uncertain' | 'confirmed' | 'rejected' | 'unchanged';
  observed: boolean | null;
}
export interface CoreFailure {
  code: string;
  message: string;
  retryable: boolean;
}
export function failure(error: unknown): CoreFailure {
  if (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    'message' in error &&
    typeof error.code === 'string' &&
    typeof error.message === 'string'
  ) {
    return {
      code: error.code,
      message: error.message,
      retryable: 'retryable' in error && error.retryable === true,
    };
  }
  return { code: 'connection', message: '无法连接音乐服务，请重新打开客户端', retryable: true };
}
