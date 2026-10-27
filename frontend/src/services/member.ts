import type { AccountData, TrackView } from './contracts';
export type MemberQuery = { kind: 'home' } | { kind: 'history'; date: string | null };
export interface MemberRights {
  serverNow: number;
  refreshAfterMs: number | null;
  entitlements: {
    kind: string;
    name: string;
    active: boolean;
    expiresAt: number;
    level: number | null;
  }[];
}
export type MemberTarget =
  | { kind: 'song' | 'playlist' | 'album' | 'radio'; id: string }
  | { kind: 'external'; key: string };
export interface MemberCard {
  id: string;
  title: string;
  subtitle: string;
  coverUrl: string | null;
  action: MemberTarget | null;
}
export type MemberContent =
  | { kind: 'banners' | 'albums' | 'playlists' | 'audio_books'; cards: MemberCard[] }
  | { kind: 'tracks'; tracks: TrackView[] }
  | { kind: 'rankings'; groups: { id: string; title: string; tracks: TrackView[] }[] };
export type MemberPage =
  | {
      kind: 'home';
      modules: { key: string; title: string; content: MemberContent }[];
      messages: string[];
    }
  | { kind: 'history'; dates: string[]; date: string | null; tracks: TrackView[] };
export interface MemberView {
  revision: string;
  cached: boolean;
  fresh: boolean;
  fetchedAt: number;
  warning: string | null;
  page: MemberPage;
}
export interface MemberClient {
  memberRights(epoch: string, refresh: boolean): Promise<AccountData<MemberRights>>;
  memberPage(epoch: string, query: MemberQuery, refresh: boolean): Promise<AccountData<MemberView>>;
  openMemberLink(epoch: string, revision: string, key: string): Promise<void>;
}
