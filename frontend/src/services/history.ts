import type { AccountData, TrackView } from './contracts';
import type { AlbumCard } from './catalog';
import type { EpisodeView, PodcastCard } from './podcast';
export type HistoryKind = 'song' | 'album' | 'playlist' | 'voice' | 'podcast' | 'audiobook';
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
  historyPage(
    epoch: string,
    kind: HistoryKind,
    refresh: boolean,
  ): Promise<AccountData<HistoryPage>>;
  historyCommand(
    epoch: string,
    kind: HistoryKind,
    command: HistoryCommand,
  ): Promise<AccountData<HistoryPage>>;
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
