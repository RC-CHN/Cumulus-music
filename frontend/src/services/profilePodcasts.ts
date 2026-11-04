import type { AccountData } from './contracts';
import type { PodcastCard } from './podcast';

export type ProfilePodcastQuery = { kind: 'created'; offset: number } | { kind: 'collected' };
export interface ProfilePodcastRow {
  card: PodcastCard;
  subscribers: number;
}
export interface ProfilePodcastPage {
  rows: ProfilePodcastRow[];
  total: number;
  offset: number;
  nextOffset: number | null;
  truncated: boolean;
}
export interface ProfilePodcastClient {
  profilePodcasts(
    epoch: string,
    uid: string,
    query: ProfilePodcastQuery,
  ): Promise<AccountData<ProfilePodcastPage | null>>;
  openProfilePodcastManagement(epoch: string, uid: string): Promise<void>;
}
