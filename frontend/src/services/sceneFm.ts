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
  | { kind: 'play' | 'toggle' | 'cancel'; revision: string }
  | { kind: 'open_voice'; revision: string; voice_id: string }
  | { kind: 'home_automatic'; revision: string; visit: string };
export interface SceneFmClient {
  sceneFmHomeEnter(epoch: string): Promise<AccountData<{ visit: string } | null>>;
  sceneFmHomeLeave(epoch: string, visit: string): Promise<void>;
  sceneFmView(epoch: string, refresh: boolean): Promise<AccountData<SceneFmView>>;
  sceneFmCommand(epoch: string, command: SceneFmCommand): Promise<AccountData<SceneFmView>>;
}
export interface SceneFmAutoplayClient {
  sceneFmAutoplayPreference(epoch: string): Promise<AccountData<boolean>>;
  setSceneFmAutoplayPreference(epoch: string, enabled: boolean): Promise<AccountData<boolean>>;
}
