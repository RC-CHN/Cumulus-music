import type { AccountData, TrackView } from './contracts';
import type { Quality } from './player';
export type FmSource =
  | { kind: 'personal' }
  | { kind: 'familiar'; source_id: string }
  | { kind: 'zone'; rule_id: string; trial_mode: number | null };
export type FmDiscoveryQuery =
  | { kind: 'home' }
  | { kind: 'zone'; page_code: string; tab_key: string };
export interface FmCard {
  source: FmSource;
  title: string;
  subtitle: string;
  coverUrl: string | null;
}
export interface FmDiscovery {
  query: FmDiscoveryQuery;
  fetchedAt: number;
  page: {
    cards: FmCard[];
    zones: { pageCode: string; name: string }[];
    tabs: { key: string; name: string }[];
    activeTab: string | null;
    defaultFallback: boolean;
  };
}
export interface FmFeedback {
  id: string;
  kind: 'skip' | 'trash' | 'artist';
  phase: string;
  desired: boolean | null;
  canApply: boolean;
  artistId: string | null;
}
export interface FmView {
  source: FmSource;
  activeSource: FmSource | null;
  revision: string | null;
  items: { entryId: string; track: TrackView }[];
  current: string | null;
  active: boolean;
  pending: boolean;
  modePending: boolean;
  notice: string | null;
  operations: FmFeedback[];
  blockedArtists: { id: string; name: string }[];
}
export interface FmReasons {
  entryId: string;
  songId: string;
  options: { key: string; label: string; artistId: string | null }[];
}
export interface FmTrash {
  page: number;
  reportedTotal: number;
  songs: TrackView[];
  more: boolean;
  cursor: string | number | null;
}
export type FmCommand =
  | { kind: 'start'; quality: Quality }
  | { kind: 'resume' | 'leave' }
  | { kind: 'trash'; entry_id: string }
  | { kind: 'feedback'; entry_id: string; reason_key: string }
  | { kind: 'artist'; id: string; blocked: boolean }
  | { kind: 'reconcile_artist'; id: string }
  | { kind: 'reconcile' | 'dispatch_skip' | 'abandon_skip'; id: string }
  | { kind: 'restore_trash'; ids: string[] };
export interface FmClient {
  fmDiscovery(
    epoch: string,
    query: FmDiscoveryQuery,
    refresh: boolean,
  ): Promise<AccountData<FmDiscovery>>;
  fmReasons(epoch: string, source: FmSource, entryId: string): Promise<AccountData<FmReasons>>;
  fmView(epoch: string, source: FmSource, refresh: boolean): Promise<AccountData<FmView>>;
  fmCommand(epoch: string, source: FmSource, command: FmCommand): Promise<AccountData<FmView>>;
  fmTrash(
    epoch: string,
    page: number,
    cursor: string | number | null,
  ): Promise<AccountData<FmTrash>>;
}
