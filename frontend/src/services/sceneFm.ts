import type { AccountData } from './contracts';
import type { EpisodeView } from './podcast';
export interface SceneFmView {
  revision: string | null;
  activeId: string | null;
  scenes: { id: string; name: string; label: string; count: number }[];
  episode: EpisodeView | null;
  active: boolean;
  playing: boolean;
  pending: boolean;
  autoplay: boolean;
}
export type SceneFmCommand =
  | { kind: 'select'; revision: string; scene_id: string }
  | { kind: 'play' | 'toggle' | 'automatic' | 'cancel'; revision: string }
  | { kind: 'autoplay'; enabled: boolean };
export interface SceneFmClient {
  sceneFmView(epoch: string, refresh: boolean): Promise<AccountData<SceneFmView>>;
  sceneFmCommand(epoch: string, command: SceneFmCommand): Promise<AccountData<SceneFmView>>;
}
