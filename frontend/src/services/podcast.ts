import type { AccountData } from './contracts';
/** Extract only an official radio identifier; this never opens the pasted URL. */
export function podcastReference(input: string): string | null {
  const value = input.trim();
  if (/^[1-9]\d{0,19}$/.test(value)) return value;
  if (value.length > 2048) return null;
  try {
    const url = new URL(value);
    if (
      !['http:', 'https:'].includes(url.protocol) ||
      url.hostname !== 'music.163.com' ||
      url.username ||
      url.password ||
      url.port
    )
      return null;
    const route = url.hash.startsWith('#/') ? new URL(url.hash.slice(1), url.origin) : url;
    if (route.pathname !== '/djradio' || route.searchParams.getAll('id').length !== 1) return null;
    const id = route.searchParams.get('id') ?? '';
    return /^[1-9]\d{0,19}$/.test(id) ? id : null;
  } catch {
    return null;
  }
}
export type PodcastScope =
  | { kind: 'podcasts' | 'audiobooks' | 'created' | 'purchased' | 'liked' }
  | { kind: 'radio'; id: string; ascending: boolean };
export type PodcastAction = 'subscribe' | 'like' | 'resume';
export interface PodcastCard {
  id: string;
  name: string;
  coverUrl: string | null;
  creator: string;
  description: string;
  programCount: number;
  subscribed: boolean | null;
  feeType: number;
  unavailable: boolean;
}
export interface EpisodeView {
  id: string;
  name: string;
  coverUrl: string | null;
  creator: string;
  radioId: string | null;
  radioName: string;
  durationMs: number;
  feeType: number;
  liked: boolean;
  unavailable: boolean;
}
export interface PodcastQuery {
  scope: PodcastScope;
  keyword: string;
  offset: number;
  limit: number;
  revision: string | null;
}
export interface PodcastMutation {
  action: PodcastAction;
  id: string;
  desired: boolean;
  observed: boolean | null;
  phase: 'sending' | 'verifying' | 'uncertain' | 'confirmed' | 'rejected' | 'unchanged';
  acknowledged: boolean;
}
export interface PodcastPage {
  epoch: string;
  scope: PodcastScope;
  revision: string;
  cards: PodcastCard[];
  episodes: EpisodeView[];
  total: number;
  offset: number;
  remoteTotal: number;
  radio: PodcastCard | null;
  likedIds: string[];
  resumeEnabled: boolean | null;
  operations: PodcastMutation[];
}
export type PodcastCommand =
  | { kind: 'change'; action: PodcastAction; id: string; desired: boolean }
  | { kind: 'reconcile'; action: PodcastAction; id: string };
export interface PodcastState {
  likedIds: string[] | null;
  operations: PodcastMutation[];
}
export interface PodcastClient {
  podcastState(epoch: string, refresh: boolean): Promise<AccountData<PodcastState>>;
  podcastPage(epoch: string, query: PodcastQuery, refresh: boolean): Promise<PodcastPage>;
  podcastCommand(epoch: string, command: PodcastCommand): Promise<AccountData<PodcastMutation>>;
}
