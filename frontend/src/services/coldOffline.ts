export interface ColdOfflineItem {
  id: string;
  title: string;
  artist: string;
  actualQuality: string;
}

export interface ColdOfflinePage {
  items: ColdOfflineItem[];
  hasMore: boolean;
}

export interface ColdOfflinePlayback {
  phase: 'idle' | 'playing';
  playingId: string | null;
}

export interface ColdOfflineClient {
  coldOfflineOpen(): Promise<ColdOfflinePage>;
  coldOfflineNext(): Promise<ColdOfflinePage>;
  coldOfflinePlay(id: string): Promise<ColdOfflinePlayback>;
  coldOfflineStop(): Promise<ColdOfflinePlayback>;
  coldOfflineStatus(): Promise<ColdOfflinePlayback>;
  coldOfflineClose(): Promise<void>;
}
