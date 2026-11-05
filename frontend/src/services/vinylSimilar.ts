import type { TrackView } from './contracts';
import type { PlayerView, Quality } from './player';

export type VinylSimilarLane = 'songs' | 'playlists';
export type VinylSimilarReadState = 'idle' | 'loading' | 'ready' | 'error';
export type VinylDetailTab = 'lyrics' | 'similar';
const tabKey = 'cumulus.playback.vinyl-tab.v1';

export function readVinylDetailTab(): VinylDetailTab {
  try {
    if (localStorage.getItem(tabKey) === 'similar') return 'similar';
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
  receipt: string | null;
  songs: VinylSimilarSong[];
}

export interface VinylRelatedPlaylists {
  playId: string;
  songId: string;
  playlists: VinylRelatedPlaylist[];
}

export interface VinylSimilarClient {
  playerVinylSimilarAvailable(epoch: string): Promise<boolean>;
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
