import type { AccountData, PlaylistView, TrackView } from './contracts';
export type PlaylistDialogTarget =
  | { kind: 'create' | 'history' | 'open' }
  | { kind: 'track'; track: TrackView; playlist?: PlaylistView };
export interface PlaylistEditor {
  id: string;
  revision: string;
  name: string;
  description: string;
  coverUrl: string | null;
  tags: string[];
  owned: boolean;
  favorite: boolean;
  private: boolean;
  trackIds: string[];
  hasLocal: boolean;
}
export interface PlaylistOrderView {
  revision: string;
  created: string[];
  subscribed: string[];
}
export type PlaylistPhase =
  | 'sending'
  | 'verifying'
  | 'uncertain'
  | 'confirmed'
  | 'rejected'
  | 'partial'
  | 'unchanged';
export type PlaylistCommand =
  | { kind: 'create'; name: string; private: boolean }
  | {
      kind: 'metadata';
      id: string;
      revision: string;
      name: string;
      tags: string[];
      description: string;
    }
  | {
      kind: 'tracks';
      id: string;
      revision: string;
      change: 'add' | 'delete' | 'reorder' | 'clear';
      ids: string[];
    }
  | { kind: 'delete' | 'publish'; id: string; revision: string }
  | { kind: 'subscribe'; id: string; subscribed: boolean }
  | { kind: 'order'; group: 'created' | 'subscribed'; revision: string; ids: string[] };
export interface PlaylistMutationView {
  operationId: string;
  kind: string;
  playlistId: string | null;
  phase: PlaylistPhase;
  label: string;
  serviceCodes: Record<string, number>;
  message: string;
  observed: PlaylistEditor | null;
}
export interface PlaylistClient {
  playlistTags(epoch: string): Promise<AccountData<{ name: string; category: string }[]>>;
  playlistEditor(epoch: string, id: string): Promise<AccountData<PlaylistEditor>>;
  playlistOrderView(epoch: string): Promise<AccountData<PlaylistOrderView>>;
  mutatePlaylist(
    epoch: string,
    command: PlaylistCommand,
  ): Promise<AccountData<PlaylistMutationView>>;
  reconcilePlaylist(epoch: string, operationId: string): Promise<AccountData<PlaylistMutationView>>;
  playlistOperations(epoch: string): Promise<AccountData<PlaylistMutationView[]>>;
}
export const playlistPending = (phase: PlaylistPhase) =>
  ['sending', 'verifying', 'uncertain'].includes(phase);
export const playlistSaved = (phase: PlaylistPhase) =>
  phase === 'confirmed' || phase === 'unchanged';
export function playlistLength(value: string) {
  let n = 0;
  for (let i = 0; i < value.length; i++) n += value.charCodeAt(i) <= 255 ? 1 : 2;
  return n;
}
