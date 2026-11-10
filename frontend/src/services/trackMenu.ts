import type { TrackView } from './contracts';
import type {
  PlayerView,
  QueueCatalogMenu,
  QueueOrigin,
  QueueVoiceMenu,
  SearchQueueRow,
} from './player';
import type { MenuItem } from './menu';
import { podcastProgramPublicLink } from './podcast.ts';

export type TrackPlayGesture = 'double_click';
export interface TrackPendingToggle {
  trackId: string;
  key: string;
  pause: boolean;
  disabled: boolean;
}

export interface TrackCurrentToggle {
  trackId: string;
  local?: boolean;
  epoch: string;
  playId: string;
  queueRevision: string;
  pause: boolean;
}

// UI-only request: playback retains the originating list and its ordering.
export interface TrackMenuRequest {
  track: TrackView;
  event: MouseEvent | KeyboardEvent;
  play: () => void;
  nextOrigin?: 'search';
  searchSinglePlay?: () => boolean;
  searchGroup?: {
    epoch: string;
    download?: import('./search').SearchGroupDownloadSelection;
    allDownloadsBlocked: boolean;
    action: { anchor: SearchQueueRow; rows: SearchQueueRow[] } | undefined;
    allCopyrightBlocked: boolean;
    live: () => boolean;
  };
  queueNext?: () => Promise<PlayerView | boolean | undefined>;
  playUnavailable?: boolean;
  mixedLocal?: boolean;
  removeLocalMember?: (anchor: HTMLElement | undefined) => void;
  mixedRemoval?: {
    group: boolean;
    live: () => boolean;
    confirm: (anchor: HTMLElement | undefined) => void;
  };
  episode?: {
    live: () => boolean;
    copyrightBlocked: boolean;
    selection?: { allCopyrightBlocked: boolean };
    collectGroup?: () => void;
    downloadGroup?: { available: () => boolean; busy: () => boolean; start: () => void };
    comments: () => void;
    download?: () => void;
  };
}

export type QueueMenuAction =
  | 'play'
  | 'comments'
  | 'collect'
  | 'download'
  | 'share'
  | 'artist'
  | 'creator'
  | 'album'
  | 'source'
  | 'copy'
  | 'directory'
  | 'remove';

interface QueueRowMenuRequest {
  tracks: readonly TrackView[];
  anchor: HTMLElement;
  point?: { x: number; y: number };
  live(): boolean;
  play(): void;
  remove(): void;
  report(message: string): void;
}
export interface QueueTrackMenuRequest extends QueueRowMenuRequest {
  kind: 'catalog';
  info: QueueCatalogMenu;
  collect(): void;
  collectionLocked(): boolean;
}
export interface QueueVoiceMenuRequest extends QueueRowMenuRequest {
  kind: 'voice';
  info: QueueVoiceMenu;
}
interface QueueFileMenuRequest extends QueueRowMenuRequest {
  info: {
    epoch: string;
    revision: string;
    entryId: string;
    track: TrackView;
    origin?: QueueOrigin;
    sourceLabel?: string;
  };
  collect(): void;
  collectionLocked(): boolean;
}
type QueueLocalMenuRequest = QueueFileMenuRequest & { kind: 'local' };
type QueueDownloadMenuRequest = QueueFileMenuRequest & {
  kind: 'downloaded';
  downloadKind: 'song' | 'voice';
};
export type QueueMenuRequest =
  | QueueTrackMenuRequest
  | QueueVoiceMenuRequest
  | QueueLocalMenuRequest
  | QueueDownloadMenuRequest;

export function queueFileMenuItems(
  input: QueueLocalMenuRequest | QueueDownloadMenuRequest,
  playing: boolean,
  transportLocked: boolean,
): MenuItem[] {
  const { track, origin } = input.info;
  const artist = track.artists[0];
  const songDownload = input.kind === 'downloaded' && input.downloadKind === 'song';
  const artistTarget = songDownload && track.artistRefs?.[0]?.id;
  const voiceDownload = input.kind === 'downloaded' && input.downloadKind === 'voice';
  const sourceLabel =
    input.kind === 'local'
      ? origin?.kind === 'playlist'
        ? input.info.sourceLabel || '歌单'
        : origin?.kind === 'local'
          ? '本地音乐'
          : undefined
      : origin?.kind === 'downloads'
        ? voiceDownload
          ? '下载的声音'
          : '下载的单曲'
        : undefined;
  return [
    {
      key: 'play',
      label: playing ? '暂停' : '播放',
      icon: playing ? 'pause' : 'play',
      disabled: transportLocked || track.missing,
    },
    ...(!voiceDownload
      ? [
          { key: 'collect', label: '收藏', icon: 'star', disabled: input.collectionLocked() },
          {
            key: 'artist',
            label: artistTarget ? `歌手：${artist}` : `歌手: ${artist || '未知'}`,
            icon: 'user',
            disabled: !artistTarget,
          },
          {
            key: 'album',
            label: `专辑: ${track.album || '未知'}`,
            icon: 'album',
            disabled: !songDownload || !track.albumId,
          },
        ]
      : []),
    ...(sourceLabel ? [{ key: 'source', label: `来自：${sourceLabel}`, icon: 'source' }] : []),
    { key: 'directory', label: '打开文件所在目录', icon: 'folder' },
    { key: 'remove-divider', label: '', separator: true },
    { key: 'remove', label: '从列表中删除', icon: 'trash', disabled: transportLocked },
  ];
}

export function queueVoiceMenuItems(
  input: QueueVoiceMenu,
  group: boolean,
  playing: boolean,
  transportLocked: boolean,
): MenuItem[] {
  const { track, voiceId, commentCount, creator } = input;
  const link = !track.missing && podcastProgramPublicLink(voiceId);
  return [
    {
      key: 'play',
      label: playing ? '暂停' : '播放',
      icon: playing ? 'pause' : 'play',
      disabled: transportLocked || track.missing,
    },
    ...(!group
      ? [
          {
            key: 'comments',
            label: commentCount && commentCount > 0 ? `查看评论(${commentCount})` : '查看评论',
            icon: 'comment',
          },
          { key: 'main-divider', label: '', separator: true },
          ...(input.downloaded === false
            ? [{ key: 'download', label: '下载', icon: 'download', disabled: track.missing }]
            : []),
          ...(link ? [{ key: 'share', label: '分享', icon: 'share' }] : []),
        ]
      : []),
    {
      key: 'source',
      label: `来自：${input.sourceLabel}`,
      icon: 'source',
      disabled: !input.radioId,
    },
    ...(!group && creator
      ? [{ key: 'creator', label: `主播：${creator.name}`, icon: 'user', disabled: !creator.id }]
      : []),
    ...(!group && link ? [{ key: 'copy', label: '复制链接', icon: 'copy' }] : []),
    ...(input.directoryAvailable
      ? [{ key: 'directory', label: '打开文件所在目录', icon: 'folder' }]
      : []),
    { key: 'remove-divider', label: '', separator: true },
    { key: 'remove', label: '从列表中删除', icon: 'trash', disabled: transportLocked },
  ];
}
export function queueSongMenuItems(
  input: QueueCatalogMenu,
  group: boolean,
  playing: boolean,
  transportLocked: boolean,
): MenuItem[] {
  const { track, commentCount } = input;
  const first = track.artistRefs?.[0];
  const firstName = first?.name || track.artists[0];
  const link = songLink(track);
  return [
    {
      key: 'play',
      label: playing ? '暂停' : '播放',
      icon: playing ? 'pause' : 'play',
      disabled: transportLocked || track.missing,
    },
    ...(!group
      ? [
          {
            key: 'comments',
            label: commentCount && commentCount > 0 ? `查看评论(${commentCount})` : '查看评论',
            icon: 'comment',
          },
        ]
      : []),
    { key: 'main-divider', label: '', separator: true },
    { key: 'collect', label: '收藏', icon: 'star' },
    ...(!input.downloaded || group ? [{ key: 'download', label: '下载', icon: 'download' }] : []),
    ...(!group && link ? [{ key: 'share', label: '分享', icon: 'share' }] : []),
    {
      key: 'artist',
      label: first?.id ? `歌手：${firstName}` : `歌手: ${firstName || '未知'}`,
      icon: 'user',
      disabled: !first?.id,
    },
    {
      key: 'album',
      label: `专辑: ${track.album || '未知'}`,
      icon: 'album',
      disabled: !track.albumId,
    },
    { key: 'source', label: `来自：${input.sourceLabel}`, icon: 'source', disabled: !input.origin },
    ...(!group && link ? [{ key: 'copy', label: '复制链接', icon: 'copy' }] : []),
    ...(input.directoryReceipt
      ? [{ key: 'directory', label: '打开文件所在目录', icon: 'folder' }]
      : []),
    { key: 'remove-divider', label: '', separator: true },
    { key: 'remove', label: '从列表中删除', icon: 'trash', disabled: transportLocked },
  ];
}
export function songLink(track: TrackView) {
  return !track.local && /^[1-9][0-9]*$/.test(track.id)
    ? `https://music.163.com/song?id=${track.id}`
    : undefined;
}
