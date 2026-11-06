import type { AccountData, LikeResult, PlaylistView, TrackView } from './contracts.ts';
import type { DownloadOptions, DownloadResult } from './downloads.ts';
import type { PlayerView, Quality } from './player.ts';
import type { PlaylistPhase } from './playlists.ts';
import type { VoicePlaybackScope } from './voiceDetail.ts';

export type VoiceSongsScope = VoicePlaybackScope;

export interface VoiceSongsView {
  playId: string;
  voiceId: string;
  receipt: string | null;
  tracks: TrackView[];
  availability: { onlineEligible: boolean; copyrightBlocked: boolean }[];
}

export type VoiceSongAction =
  | { kind: 'cover'; index: number }
  | { kind: 'double_click'; index: number }
  | { kind: 'next_play'; index: number };

export interface VoiceSongSelectionAction {
  kind: 'menu_play' | 'next_play' | 'double_click';
  index: number;
  indices: readonly number[];
}

export interface VoiceSongRowTarget extends VoiceSongsScope {
  receipt: string;
  index: number;
}

export type VoiceSongDownloadDecision =
  | 'ready'
  | 'playlist_required'
  | 'membership_unknown'
  | 'complete'
  | 'existing_task'
  | 'waiting'
  | 'working'
  | 'unavailable'
  | 'extended_rights_unsupported'
  | 'cloud_unsupported';

export interface VoiceSongDownloadPlan {
  epoch: string;
  playId: string;
  trackId: string;
  decision: VoiceSongDownloadDecision;
}

export interface VoiceSongDownloadPicker {
  epoch: string;
  playId: string;
  trackId: string;
  playlistId: string;
  operationId: string;
  phase: PlaylistPhase;
  /** Definite add failure, even if readback sees a concurrent membership change. */
  addFailed: boolean;
  /** Only this opaque native value permits the picker-backed download. */
  continuation: string | null;
}

export interface VoiceSongDownloadCreation {
  epoch: string;
  playId: string;
  trackId: string;
  createOperationId: string;
  createPhase: PlaylistPhase;
  playlistId: string | null;
  add: VoiceSongDownloadPicker | null;
}

export interface VoiceSongDownloadClient {
  playerVoiceSongDownloadPrepare(target: VoiceSongRowTarget): Promise<VoiceSongDownloadPlan>;
  playerVoiceSongDownloadStart(
    target: VoiceSongRowTarget,
    options: DownloadOptions,
  ): Promise<DownloadResult>;
  playerVoiceSongDownloadChoosePlaylist(
    target: VoiceSongRowTarget,
    playlistId: string,
  ): Promise<VoiceSongDownloadPicker>;
  playerVoiceSongDownloadPickerStatus(
    target: VoiceSongRowTarget,
    reconcile: boolean,
  ): Promise<VoiceSongDownloadPicker>;
  playerVoiceSongDownloadCreatePlaylist(
    target: VoiceSongRowTarget,
    name: string,
    privatePlaylist: boolean,
  ): Promise<VoiceSongDownloadCreation>;
  playerVoiceSongDownloadCreationStatus(
    target: VoiceSongRowTarget,
    reconcile: boolean,
  ): Promise<VoiceSongDownloadCreation>;
  playerVoiceSongDownloadAddCreatedPlaylist(
    target: VoiceSongRowTarget,
  ): Promise<VoiceSongDownloadCreation>;
  playerVoiceSongDownloadStartFromPlaylist(
    target: VoiceSongRowTarget,
    continuation: string,
    options: DownloadOptions,
  ): Promise<DownloadResult>;
}

export interface VoiceSongCollectCard extends VoiceSongsScope {
  cardReceipt: string;
}

export interface VoiceSongCollectTarget extends VoiceSongCollectCard {
  selectionReceipt: string;
}

export interface VoiceSongCollectPlan {
  epoch: string;
  playId: string;
  selectionReceipt: string | null;
  selectedCount: number;
  eligibleCount: number;
  omittedIndices: number[];
  usedFirstFallback: boolean;
}

export interface VoiceSongCollectResult {
  epoch: string;
  playId: string;
  selectionReceipt: string;
  playlistId: string;
  operationId: string;
  phase: PlaylistPhase;
}

export interface VoiceSongCollectClient {
  playerVoiceSongsCollectPrepare(
    target: VoiceSongCollectCard,
    indices: readonly number[],
  ): Promise<VoiceSongCollectPlan>;
  playerVoiceSongsCollectDestinations(
    target: VoiceSongCollectTarget,
    sort: 'default' | 'recently',
    page: number,
  ): Promise<AccountData<{ playlists: PlaylistView[]; nextPage: number | null }>>;
  playerVoiceSongsCollectExisting(
    target: VoiceSongCollectTarget,
    playlistId: string,
  ): Promise<VoiceSongCollectResult>;
  playerVoiceSongsCollectStatus(
    target: VoiceSongCollectTarget,
    reconcile: boolean,
  ): Promise<VoiceSongCollectResult>;
}

export interface VoiceSongsClient extends VoiceSongDownloadClient, VoiceSongCollectClient {
  playerVoiceSongs(epoch: string, playId: string): Promise<VoiceSongsView>;
  playerVoiceSongPlay(
    epoch: string,
    playId: string,
    receipt: string,
    action: VoiceSongAction,
    quality: Quality,
  ): Promise<PlayerView>;
  playerVoiceSongsSelection(
    epoch: string,
    playId: string,
    receipt: string,
    action: VoiceSongSelectionAction,
    quality: Quality,
  ): Promise<PlayerView>;
  playerVoiceSongLike(
    epoch: string,
    playId: string,
    receipt: string,
    index: number,
    liked: boolean,
  ): Promise<AccountData<LikeResult>>;
}
