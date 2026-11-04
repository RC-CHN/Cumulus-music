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
export interface MemberSignDay {
  label: string;
  today: boolean;
  signed: boolean;
  signTime: number | null;
  coverUrl: string | null;
}
export interface MemberSignCard {
  title: string;
  subtitle: string;
  buttonText: string | null;
  textureUrl: string | null;
  days: MemberSignDay[];
  action: MemberSignAction;
}
export interface MemberSignAction {
  phase: 'ready' | 'signed' | 'unknown' | 'unavailable';
  dayKey: string | null;
}
export type MemberSignCommand = { kind: 'check_in'; dayKey: string } | { kind: 'reconcile' };
export interface MemberSignOutcome {
  action: MemberSignAction;
  card: MemberSignCard | null;
  detail: MemberSignDetail | null;
}
export interface MemberSignDetail {
  time: number;
  songId: string;
  song: string;
  artist: string | null;
  coverUrl: string | null;
  wish: string | null;
  wishAuthor: string | null;
  monthTotal: number | null;
  allRewardsClaimed: boolean;
  nextReward: { daysToGo: number; title: string; subtitle: string | null } | null;
  portalQr: { modules: boolean[][] };
}
export interface MemberClient {
  memberRights(epoch: string, refresh: boolean): Promise<AccountData<MemberRights>>;
  memberPage(epoch: string, query: MemberQuery, refresh: boolean): Promise<AccountData<MemberView>>;
  memberSignCard(epoch: string): Promise<AccountData<MemberSignCard>>;
  memberSignDetail(epoch: string, signTime: number): Promise<AccountData<MemberSignDetail>>;
  memberSignCommand(
    epoch: string,
    command: MemberSignCommand,
  ): Promise<AccountData<MemberSignOutcome>>;
  openMemberLink(epoch: string, revision: string, key: string): Promise<void>;
}
export interface MemberPortalClient {
  memberPortalQr(): Promise<{ modules: boolean[][] }>;
  memberCardOffer(epoch: string, refresh: boolean): Promise<AccountData<MemberCardOffer>>;
  openMemberCardOffer(epoch: string, revision: string): Promise<void>;
}
export interface MemberCardOffer {
  mode: 'app' | 'qr' | 'browser';
  label: string | null;
  revision: string;
}

export function validMemberQr(modules: unknown): modules is boolean[][] {
  return (
    Array.isArray(modules) &&
    modules.length >= 21 &&
    modules.length <= 41 &&
    (modules.length - 21) % 4 === 0 &&
    modules.every(
      (row) =>
        Array.isArray(row) &&
        row.length === modules.length &&
        row.every((cell) => typeof cell === 'boolean'),
    )
  );
}
