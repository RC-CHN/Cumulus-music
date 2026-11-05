import type { TrackView } from './contracts';
import type { PlayerView, Quality } from './player';

export type VinylSimilarLane = 'songs' | 'playlists';
export type VinylSimilarReadState = 'idle' | 'loading' | 'ready' | 'error';
export type VinylDetailTab = 'lyrics' | 'wiki' | 'similar';
export type VinylRecommendationPlacement = 'unresolved' | 'standalone' | 'wiki';
export type VinylRecommendationLane = Exclude<VinylRecommendationPlacement, 'unresolved'>;
const tabKey = 'cumulus.playback.vinyl-tab.v1';

export function vinylDetailTabs(
  wiki: boolean,
  placement: VinylRecommendationPlacement,
): VinylDetailTab[] {
  if (!wiki) return ['lyrics'];
  return placement === 'standalone' ? ['lyrics', 'wiki', 'similar'] : ['lyrics', 'wiki'];
}

export function activeVinylDetailTab(
  preferred: VinylDetailTab,
  wiki: boolean,
  placement: VinylRecommendationPlacement,
): VinylDetailTab {
  return vinylDetailTabs(wiki, placement).includes(preferred) ? preferred : 'lyrics';
}

export function readVinylDetailTab(): VinylDetailTab {
  try {
    const value = localStorage.getItem(tabKey);
    if (value === 'wiki' || value === 'similar') return value;
  } catch {
    // Tab choice still works for the open session.
  }
  return 'lyrics';
}

export function storeVinylDetailTab(tab: VinylDetailTab) {
  try {
    localStorage.setItem(tabKey, tab);
  } catch {
    // WebView storage is optional for this preference.
  }
}
export type VinylSimilarAction =
  | { kind: 'header' }
  | { kind: 'cover' | 'double_click'; index: number };

export interface VinylSimilarSeed {
  epoch: string;
  playId: string;
  songId: string;
}

export interface VinylSimilarSong {
  track: TrackView;
  playable: boolean;
}

export interface VinylRelatedPlaylist {
  id: string;
  name: string;
  coverUrl: string | null;
  playCount: number;
}

export interface VinylSimilarSongs {
  playId: string;
  songId: string;
  placement: VinylRecommendationLane;
  receipt: string | null;
  songs: VinylSimilarSong[];
}

export interface VinylRelatedPlaylists {
  playId: string;
  songId: string;
  placement: VinylRecommendationLane;
  playlists: VinylRelatedPlaylist[];
}

export interface VinylWikiCard {
  metas: readonly { title: string; content: string }[];
}

export interface VinylWikiView {
  playId: string;
  songId: string;
  card: VinylWikiCard | null;
}

export interface VinylSimilarClient {
  playerVinylSimilarAvailable(epoch: string): Promise<boolean>;
  playerVinylRecommendationPlacement(epoch: string): Promise<VinylRecommendationPlacement>;
  playerVinylWiki(epoch: string, playId: string): Promise<VinylWikiView>;
  playerVinylSimilarSongs(epoch: string, playId: string): Promise<VinylSimilarSongs>;
  playerVinylRelatedPlaylists(epoch: string, playId: string): Promise<VinylRelatedPlaylists>;
  playerVinylSimilarPlay(
    epoch: string,
    playId: string,
    receipt: string,
    action: VinylSimilarAction,
    quality: Quality,
  ): Promise<PlayerView>;
}
