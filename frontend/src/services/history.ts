import type { AccountData, TrackView } from './contracts';
import type { AlbumCard } from './catalog';
import type { VoiceDownloadPreparation } from './downloads';
import type { EpisodeView, PodcastCard, PodcastMutation } from './podcast';
export type HistoryKind = 'song' | 'album' | 'playlist' | 'voice' | 'podcast' | 'audiobook';
export type HistoryQueueKind = Extract<HistoryKind, 'song' | 'voice'>;
export type HistoryQueueAction =
  | { kind: 'add-all' }
  | { kind: 'play-all' }
  | { kind: 'play' | 'add'; ids: string[] };
export type HistoryRowQueueAction = 'play' | 'next';
export interface HistoryRowQueueTarget {
  kind: HistoryQueueKind;
  revision: string;
  clickedId: string;
  selectedIds: string[];
  epoch: string;
  routeIdentity: object;
}
export type HistoryRowQueueOutcome = 'queued' | 'playing';
export interface HistoryRowMenuTarget {
  history_kind: HistoryQueueKind;
  revision: string;
  clicked_id: string;
  selected_ids: string[];
}
export type HistorySongRowDownloadTarget = HistoryRowMenuTarget & { history_kind: 'song' };
export interface HistoryRowMenuInfo {
  comment: {
    resource: { kind: 'track' | 'voice'; id: string };
    count: number | null;
  } | null;
  copyLink: string | null;
  canShare?: boolean;
  songDownload?: { kind: 'download' | 'copyright_blocked' | 'unavailable' };
  songFile?: { kind: 'reveal'; receipt: string };
  voiceFile?: { kind: 'download' } | { kind: 'reveal'; receipt: string } | { kind: 'unavailable' };
}
export type HistoryRowShareChoice = 'copy' | 'wechat_qr';
export type HistoryRowSharePreparation =
  | { kind: 'ready'; receipt: string }
  | { kind: 'login_required' | 'unavailable' | 'album_required' };
export type HistoryRowShareOutcome =
  | { kind: 'copy'; url: string }
  | { kind: 'wechat_qr'; modules: boolean[][] }
  | { kind: 'unavailable' | 'album_required' };
export interface HistoryRowMenuClient {
  historyRowSharePrepare(
    epoch: string,
    target: HistoryRowMenuTarget,
  ): Promise<AccountData<HistoryRowSharePreparation>>;
  historyRowShareChoose(
    epoch: string,
    target: HistoryRowMenuTarget,
    receipt: string,
    choice: HistoryRowShareChoice,
  ): Promise<AccountData<HistoryRowShareOutcome>>;
  historyRowMenuInfo(
    epoch: string,
    target: HistoryRowMenuTarget,
  ): Promise<AccountData<HistoryRowMenuInfo>>;
  historyRowShareCancel(
    epoch: string,
    target: HistoryRowMenuTarget,
    receipt: string,
  ): Promise<void>;
  downloadHistoryRowVoicePrepare(
    epoch: string,
    target: HistoryRowMenuTarget,
  ): Promise<VoiceDownloadPreparation>;
  historyRowSongReveal(
    epoch: string,
    target: HistoryRowMenuTarget & { history_kind: 'song' },
    receipt: string,
  ): Promise<void>;
  historyRowVoiceReveal(
    epoch: string,
    target: HistoryRowMenuTarget,
    receipt: string,
  ): Promise<void>;
}
export interface HistorySelectionScope {
  revision: string;
  ids: string[];
}
export type HistoryCollectRowTarget = { kind: 'row' } & Omit<HistoryRowMenuTarget, 'history_kind'>;
export type HistoryCollectTarget =
  | { kind: 'all'; revision: string }
  | ({ kind: 'selected' } & HistorySelectionScope)
  | HistoryCollectRowTarget;
export interface HistoryCollectPlan {
  receipt: string | null;
  canCreate: boolean;
  selectedCount: number;
  songIds: string[];
}
export type HistoryRadioKind = Extract<HistoryKind, 'podcast' | 'audiobook'>;
export type HistoryRadioMenuAction = 'play' | 'next';
export const historyTabs: { kind: HistoryKind; label: string }[] = [
  { kind: 'song', label: '歌曲' },
  { kind: 'podcast', label: '播客' },
  { kind: 'audiobook', label: '有声书' },
  { kind: 'voice', label: '声音' },
  { kind: 'playlist', label: '歌单' },
  { kind: 'album', label: '专辑' },
];
export interface HistoryPlaylistCard {
  id: string;
  name: string;
  description: string;
  creator: string;
  coverUrl: string | null;
  trackCount: number;
}
export type HistoryResource =
  | { kind: 'song'; track: TrackView }
  | { kind: 'album'; album: AlbumCard }
  | { kind: 'playlist'; playlist: HistoryPlaylistCard }
  | { kind: 'voice'; voice: EpisodeView }
  | { kind: 'podcast' | 'audiobook'; radio: PodcastCard };
export interface HistoryEntry {
  id: string;
  playedAt: number;
  device: string;
  origin: 'local' | 'remote';
  playProgress: number | null;
  listened: boolean | null;
  resource: HistoryResource;
  songMenuCopyrightBlocked?: boolean;
  canCollectRadio?: boolean;
  chapter?: {
    name: string;
    tag: string | null;
    recent: boolean;
    percent: number | null;
    completed: boolean;
    playable: boolean;
  };
}
export interface HistoryOperation {
  id: string;
  action: 'clear' | 'delete';
  selectedCount: number | null;
  phase:
    | 'pending'
    | 'confirmed'
    | 'rejected'
    | 'uncertain'
    | 'accepted_current_state'
    | 'cancelled';
  createdAt: number;
  localRemoved: number;
  remotePlanned: boolean;
  lastReadAt: number | null;
}
export interface HistoryPage {
  kind: HistoryKind;
  revision: string;
  chapterRevision?: string;
  syncEnabled: boolean;
  canSync: boolean;
  remoteIncluded: boolean;
  remoteFetchedAt: number | null;
  localCount: number;
  entries: HistoryEntry[];
  operations: HistoryOperation[];
}
export type HistoryAction = { kind: 'clear' } | { kind: 'delete'; ids: string[] };
export type HistoryCommand =
  | { kind: 'cancel_unseen'; id: string; expected_revision: string; action: HistoryAction }
  | { kind: 'set_sync'; enabled: boolean }
  | { kind: 'mutate'; id: string; expected_revision: string; action: HistoryAction }
  | { kind: 'reconcile'; id: string }
  | { kind: 'accept_current'; id: string; expected_revision: string };
export interface HistoryClient {
  listenedBooks(epoch: string, query: ListenedBooksQuery): Promise<AccountData<ListenedBooksPage>>;
  historyPage(
    epoch: string,
    kind: HistoryKind,
    refresh: boolean,
  ): Promise<AccountData<HistoryPage>>;
  historyCollectPrepare(
    epoch: string,
    target: HistoryCollectTarget,
  ): Promise<AccountData<HistoryCollectPlan>>;
  historyCommand(
    epoch: string,
    kind: HistoryKind,
    command: HistoryCommand,
  ): Promise<AccountData<HistoryPage>>;
}
export type HistoryRadioCollection = { kind: 'toggle'; expected: boolean } | { kind: 'reconcile' };
export interface HistoryRadioCollectionClient {
  historyRadioCollection(
    epoch: string,
    kind: HistoryRadioKind,
    revision: string,
    id: string,
    action: HistoryRadioCollection,
  ): Promise<AccountData<PodcastMutation>>;
}
export interface HistorySyncState {
  enabled: boolean;
  firstDisableConfirmed: boolean;
  revision: string;
}
export type HistorySyncAction =
  | { kind: 'read' }
  | { kind: 'set'; enabled: boolean; confirmed_first_disable: boolean };
export interface HistorySyncClient {
  onHistorySyncChanged(
    listener: (change: AccountData<HistorySyncState>) => void,
  ): Promise<() => void>;
  historySyncPreference(
    epoch: string,
    action: HistorySyncAction,
  ): Promise<AccountData<HistorySyncState>>;
}
export const historyPending = (op: HistoryOperation) =>
  op.phase === 'pending' || op.phase === 'uncertain';

export function historyOutcome(op: HistoryOperation) {
  switch (op.phase) {
    case 'pending':
      return '请求尚未确认，请先核对记录';
    case 'uncertain':
      return '服务端结果未知，请先核对记录';
    case 'rejected':
      return `服务端未完成操作；本机已移除 ${op.localRemoved} 条记录`;
    case 'accepted_current_state':
      return '已保留重新读取的状态，未重发原请求';
    case 'cancelled':
      return '未开始的请求已取消';
    default:
      return op.remotePlanned ? '服务端已确认，本机记录已更新' : '本机记录已更新';
  }
}
export function historyLabel(entry: HistoryEntry) {
  const r = entry.resource;
  switch (r.kind) {
    case 'song':
      return {
        name: r.track.name,
        subtitle: r.track.artists.join(' / '),
        cover: r.track.coverUrl,
        available: !r.track.missing,
      };
    case 'album':
      return {
        name: r.album.name,
        subtitle: r.album.artists.map((a) => a.name).join(' / '),
        cover: r.album.coverUrl,
        available: true,
      };
    case 'playlist':
      return {
        name: r.playlist.name,
        subtitle: r.playlist.creator,
        cover: r.playlist.coverUrl,
        available: true,
      };
    case 'voice':
      return {
        name: r.voice.name,
        subtitle: r.voice.radioName || r.voice.creator,
        cover: r.voice.coverUrl,
        available: !r.voice.unavailable,
      };
    default:
      return {
        name: r.radio.name,
        subtitle: r.radio.creator,
        cover: r.radio.coverUrl,
        available: !r.radio.unavailable,
      };
  }
}
/** Official selectedData uses the displayed order; native checks admission. */
export function selectedHistoryBatch(
  kind: HistoryKind,
  entries: readonly HistoryEntry[],
  selected: ReadonlySet<string>,
): string[] {
  if (kind !== 'song' && kind !== 'voice') return [];
  return entries
    .filter((entry) => entry.resource.kind === kind && selected.has(entry.id))
    .map((entry) => entry.id);
}
export function historyTime(value: number, now = new Date()) {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime()) || value <= 0)
    return { short: '时间未知', full: '时间未知' };
  const day = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1).getTime();
  const time = date.toLocaleTimeString('zh-CN', {
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  });
  return {
    short:
      day === today
        ? `今天 ${time}`
        : day === yesterday
          ? `昨天 ${time}`
          : date.toLocaleDateString('zh-CN'),
    full: date.toLocaleString('zh-CN', { hour12: false }),
  };
}

export interface ListenedBooksPage {
  revision: string;
  number: number;
  pages: number;
  total: number;
  rows: {
    key: number;
    radio: PodcastCard;
    subscribers: number | null;
    chapter: {
      name: string;
      recent: boolean;
      percent: number | null;
      completed: boolean;
      playable: boolean;
    } | null;
  }[];
}
export type ListenedBooksQuery =
  | { kind: 'open' }
  | { kind: 'page'; revision: string; number: number };
