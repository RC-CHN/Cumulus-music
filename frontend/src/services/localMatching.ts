import type { AccountData, TrackView } from './contracts';
import type { LocalQuery } from './local';
export interface MatchJob {
  id: string;
  phase: 'queued' | 'running' | 'paused' | 'failed' | 'complete' | 'cancelled';
  total: number;
  processed: number;
  matched: number;
  unmatched: number;
  skipped: number;
  revision: number;
  error: string | null;
}
export interface LocalLink {
  id: string;
  revision: string;
  songId: string;
  track: TrackView | null;
  source: 'file_metadata' | 'search' | 'manual' | null;
}
export interface LocalMatchView {
  job: MatchJob | null;
  links: LocalLink[];
}
export type MatchCommand =
  | { kind: 'start'; query: LocalQuery }
  | { kind: 'control'; id: string; action: 'pause' | 'resume' | 'cancel' }
  | { kind: 'bind'; id: string; revision: string; song_id: string }
  | { kind: 'clear'; id: string; revision: string };
export interface LocalMatchClient {
  localMatchView(epoch: string, ids: string[]): Promise<AccountData<LocalMatchView>>;
  localMatchCommand(epoch: string, command: MatchCommand): Promise<void>;
}
