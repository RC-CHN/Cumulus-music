import type { AccountData, StyleSeedHandle } from './contracts';
import type { RankingGroup } from './rankings';

export type DiscoveryHomeTarget =
  | { kind: 'song' | 'playlist' | 'album' | 'podcast' | 'voice'; id: string }
  | { kind: 'fm' | 'daily' }
  | { kind: 'style'; seed: StyleSeedHandle };

export interface DiscoveryHomeCard {
  title: string;
  subtitle: string | null;
  coverUrl: string | null;
  target: DiscoveryHomeTarget;
}

export interface DiscoveryHomeSection {
  kind: string;
  title: string;
  cards: DiscoveryHomeCard[];
}

export interface DiscoveryHomePage {
  recommendationEnabled: boolean;
  sections: DiscoveryHomeSection[];
}

export interface DiscoveryHomeClient {
  discoveryHome(epoch: string): Promise<AccountData<DiscoveryHomePage>>;
  discoveryHomeRankings(epoch: string): Promise<AccountData<RankingGroup | null>>;
}
