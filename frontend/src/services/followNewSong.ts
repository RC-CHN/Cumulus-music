import type { AccountData, TrackView } from './contracts';

export interface FollowRelease {
  kind: 'song' | 'album';
  publishTime: number;
  artistId: string;
  artistName: string;
  artistImageUrl: string | null;
  resourceId: string | null;
  resourceName: string | null;
  resourceImageUrl: string | null;
  albumId: string | null;
  albumSongCount: number | null;
  tracks: { song: TrackView; mvId: string | null }[];
}

export interface FollowNewSongPage {
  releases: FollowRelease[];
  nextTimestamp: number | null;
  latestVisitTime: number | null;
}

export interface FollowLatest {
  revision: string;
  tracks: TrackView[];
}

export interface FollowNewSongClient {
  followedNewSongPage(
    epoch: string,
    startTimestamp: number | null,
  ): Promise<AccountData<FollowNewSongPage>>;
  followedNewSongLatest(epoch: string): Promise<AccountData<FollowLatest>>;
}

export type ReleaseNotificationState = 'ready' | 'confirmed' | 'uncertain';
export interface ReleaseNotificationClient {
  releaseNotification(
    epoch: string,
    artist: string,
    action: 'inspect' | 'disable',
  ): Promise<AccountData<ReleaseNotificationState>>;
}
