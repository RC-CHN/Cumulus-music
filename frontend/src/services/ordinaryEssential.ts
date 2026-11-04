import type { MenuItem } from './menu';
import type { AccountData } from './contracts';

export type EssentialCategory = 0 | 1 | 2 | 3 | 4;
export type EssentialRail = 'playlists' | 'songs' | 'books' | 'voices';

export interface EssentialCard {
  id: string;
  title: string;
  subtitle: string | null;
  coverUrl: string | null;
  radioId: string | null;
  history: boolean;
  playlistMenu?: { trackCount: number | null; hasTracks: boolean | null; isPublic: boolean };
}

export interface EssentialBooks {
  title: string;
  cards: EssentialCard[];
}

export interface OrdinaryEssentialContent {
  playlists: EssentialCard[];
  songs: EssentialCard[];
  books: EssentialBooks | null;
  voices: EssentialCard[];
}

export interface OrdinaryEssentialView {
  content: OrdinaryEssentialContent;
  failedBlocks: number;
}

export interface OrdinaryEssentialClient {
  ordinaryEssential(
    epoch: string,
    category: EssentialCategory,
  ): Promise<AccountData<OrdinaryEssentialView>>;
  ordinaryEssentialBooks(
    epoch: string,
    category: EssentialCategory,
  ): Promise<AccountData<EssentialBooks | null>>;
}

/** M/622 ordinary card branch: no edit, removal, batch or sidebar order. */
export function essentialPlaylistMenu(
  card: EssentialCard,
  state: { known?: boolean; playable: boolean; queueable: boolean; fm?: boolean; busy: boolean },
): MenuItem[] {
  const empty = card.playlistMenu?.hasTracks === false;
  const linksBlocked = empty || card.playlistMenu?.isPublic !== true;
  return [
    { key: 'view', label: '查看歌单', icon: 'list' },
    { key: 'play', label: '播放', icon: 'play', disabled: empty || !state.playable },
    {
      key: 'next',
      label: '下一首播放',
      icon: 'next',
      disabled: empty || !state.queueable || state.busy,
      hint: state.fm ? '私人漫游期间请先切换到普通播放队列' : undefined,
    },
    { key: 'resource-divider', label: '', separator: true },
    ...(state.known === false
      ? [{ key: 'subscribe', label: '收藏', icon: 'star', disabled: state.busy }]
      : []),
    { key: 'share', label: '分享…', icon: 'share', disabled: linksBlocked },
    { key: 'copy', label: '复制链接', icon: 'copy', disabled: linksBlocked },
    { key: 'download', label: '下载全部', icon: 'download', disabled: state.busy },
  ];
}
