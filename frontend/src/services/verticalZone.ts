import type { AccountData, PageRequest, TrackPage } from './contracts';
import type { PlayerView, Quality } from './player';

export type ZoneFmCardGesture = 'cover' | 'overlay';

export type VerticalZoneLocation = {
  pageCode: string;
  tabKey?: string;
  tabKeyName?: string;
  from?: 'searchHotExplain';
};

export interface VerticalZoneMenuEntry {
  key: string;
  name: string;
  pageCode: string;
}

export interface VerticalZoneMenu {
  available: boolean;
  defaultZoneTags: VerticalZoneMenuEntry[];
  moreZoneTags: VerticalZoneMenuEntry[];
}

export interface VerticalZoneTab {
  key: string;
  name: string;
}

export type VerticalZoneTarget =
  | { kind: 'song' | 'album' | 'artist' | 'user' | 'podcast' | 'voice'; id: string }
  | { kind: 'playlist' | 'fm'; id: string; personalized: boolean };

export interface VerticalZoneCard {
  cardKey: string;
  title: string | null;
  cover: string | null;
  target: VerticalZoneTarget;
  canPlay: boolean;
  previews: { title: string; artist: string | null }[];
}

export interface VerticalZoneBlock {
  kind: 'carousel' | 'horizontal' | 'ranking' | 'grid';
  title: string | null;
  requiresRecommendation: boolean;
  cards: VerticalZoneCard[];
}

export interface VerticalZoneContent {
  viewId: string;
  layout: 'classic' | 'tabs' | 'fallback';
  tabs: VerticalZoneTab[];
  activeTab: VerticalZoneTab | null;
  cardSize: 'default' | 'small' | 'mini';
  blocks: VerticalZoneBlock[];
}

export interface ZonePlaylistOrigin {
  receipt: string;
}

export interface ZonePlaylistBatchOrigin extends ZonePlaylistOrigin {
  revision: string;
}

export interface VerticalZoneClient {
  verticalZoneFmCard(
    epoch: string,
    viewId: string,
    cardKey: string,
    gesture: ZoneFmCardGesture,
    quality: Quality,
  ): Promise<AccountData<{ playback: PlayerView; detailPlayId: string | null }>>;
  verticalZoneFmPreview(
    epoch: string,
    viewId: string,
    cardKey: string,
  ): Promise<AccountData<{ coverUrl: string | null }>>;
  verticalZonePlaylistOrigin(
    epoch: string,
    viewId: string,
    cardKey: string,
    id: string,
  ): Promise<AccountData<ZonePlaylistOrigin>>;
  verticalZonePlaylistDetail(
    epoch: string,
    receipt: string,
    id: string,
    page: PageRequest,
  ): Promise<AccountData<TrackPage>>;
  verticalZonePlaylistBatchPage(
    epoch: string,
    receipt: string,
    id: string,
    page: PageRequest,
  ): Promise<AccountData<TrackPage>>;
  verticalZoneMenu(epoch: string): Promise<AccountData<VerticalZoneMenu>>;
  verticalZonePage(
    epoch: string,
    pageCode: string,
    tabKey?: string,
    tabKeyName?: string,
  ): Promise<AccountData<VerticalZoneContent>>;
}
