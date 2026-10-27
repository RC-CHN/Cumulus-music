import type { AccountData, TrackView } from './contracts';
export interface MvCard {
  id: string;
  title: string;
  artists: string[];
  coverUrl: string | null;
  durationMs: number;
  playCount: number;
  subscribed: boolean;
}
export interface MvDownloadChoice {
  resolution: number;
  estimatedBytes: number | null;
  points: number;
  available: boolean;
  reason: string | null;
}
export interface MvDetail {
  card: MvCard;
  description: string;
  publishedDate: string;
  playResolutions: number[];
  downloads: MvDownloadChoice[];
  cached: boolean;
  liked: boolean | null;
  likeCount: number;
  commentCount: number;
  likeOperation: MvLikeOperation | null;
  relatedSongs: TrackView[];
  relatedPrivilegeFailed: boolean;
}
export interface MvLikeOperation extends MvOperation {
  count: number | null;
}
export interface MvRecommendations {
  id: string;
  items: Omit<MvCard, 'subscribed'>[];
  cached: boolean;
}
export interface MvQuery {
  keyword: string;
  offset: number;
  limit: number;
  revision: string | null;
}
export interface MvOperation {
  id: string;
  desired: boolean;
  observed: boolean | null;
  phase: 'sending' | 'verifying' | 'uncertain' | 'confirmed' | 'rejected' | 'unchanged';
}
export interface MvPage {
  epoch: string;
  revision: string;
  cards: MvCard[];
  total: number;
  offset: number;
  operations: MvOperation[];
}
export interface VideoView {
  epoch: string;
  revision: string;
  playId: string;
  id: string | null;
  state:
    | 'idle'
    | 'resolving'
    | 'loading'
    | 'file_operation'
    | 'playing'
    | 'paused'
    | 'ended'
    | 'error';
  requestedResolution: number;
  actualResolution: number | null;
  local: boolean;
  position: number | null;
  duration: number | null;
  paused: boolean;
  buffering: boolean;
  volume: number;
  speed: number;
  error: string | null;
}
export type VideoAction =
  | { kind: 'open'; id: string; resolution: number; download_task_id?: string }
  | { kind: 'play' | 'refresh' | 'stop' }
  | { kind: 'pause'; paused: boolean }
  | { kind: 'seek'; seconds: number }
  | { kind: 'volume' | 'speed'; value: number }
  | { kind: 'quality'; resolution: number };
export interface VideoFrame {
  width: number;
  height: number;
  sequence: bigint;
  pixels: Uint8ClampedArray<ArrayBuffer>;
}
export interface MvClient {
  mvPage(epoch: string, query: MvQuery, refresh: boolean): Promise<MvPage>;
  mvDetail(epoch: string, id: string): Promise<AccountData<MvDetail>>;
  mvCollect(epoch: string, id: string, desired: boolean): Promise<AccountData<MvOperation>>;
  mvReconcile(epoch: string, id: string): Promise<AccountData<MvOperation>>;
  mvLike(epoch: string, id: string, desired: boolean): Promise<AccountData<MvLikeOperation>>;
  mvLikeReconcile(epoch: string, id: string): Promise<AccountData<MvLikeOperation>>;
  mvRecommendations(
    epoch: string,
    id: string,
    refresh: boolean,
  ): Promise<AccountData<MvRecommendations>>;
  videoSnapshot(epoch: string): Promise<VideoView>;
  videoAction(epoch: string, action: VideoAction): Promise<VideoView>;
  videoFrame(
    epoch: string,
    playId: string,
    after: string,
    width: number,
    height: number,
  ): Promise<VideoFrame | null>;
}
export function decodeVideoFrame(buffer: ArrayBuffer): VideoFrame | null {
  if (buffer.byteLength === 0) return null;
  if (buffer.byteLength < 16) throw new Error('Invalid video frame header');
  const header = new DataView(buffer),
    width = header.getUint32(0, true),
    height = header.getUint32(4, true),
    sequence = header.getBigUint64(8, true);
  if (
    width < 16 ||
    width > 1920 ||
    height < 16 ||
    height > 1080 ||
    buffer.byteLength !== 16 + width * height * 4 ||
    sequence === 0n
  )
    throw new Error('Invalid video frame dimensions');
  return { width, height, sequence, pixels: new Uint8ClampedArray(buffer, 16) };
}
export function videoTime(value: number | null) {
  const seconds = Math.max(0, Math.floor(value ?? 0));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}
