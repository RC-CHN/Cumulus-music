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
  availability: VoiceSongRowAvailability[];
}

export interface VoiceSongRowAvailability {
  onlineEligible: boolean;
  copyrightBlocked: boolean;
  /** A card-local affordance, never an admission receipt. */
  groupDownload?: 'ordinary' | 'omitted' | 'unsupported';
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

/** Exact M/3051 choices; the native plan further restricts each row's subset. */
export type VoiceSongPurchasedQuality = Extract<
  Quality,
  'vivid' | 'sky' | 'jymaster' | 'jyeffect' | 'hires' | 'lossless' | 'exhigh' | 'standard'
>;

declare const purchaseSelection: unique symbol;
declare const purchaseContinuation: unique symbol;

/** Memory-only round-trip to Rust, never derived from an ID, rights or the playlist operation. */
export type VoiceSongPurchasedSelection = string & { readonly [purchaseSelection]: true };
export type VoiceSongPurchasedContinuation = string & { readonly [purchaseContinuation]: true };

export type VoiceSongPurchasedDownloadPlan = Omit<VoiceSongDownloadPlan, 'decision'> &
  (
    | {
        decision: 'purchased_quality_required';
        purchasedQualities: readonly [VoiceSongPurchasedQuality, ...VoiceSongPurchasedQuality[]];
      }
    | { decision: VoiceSongDownloadDecision; purchasedQualities?: never }
  );

export type VoiceSongPurchasedDownloadOptions = Omit<DownloadOptions, 'quality'> & {
  quality: VoiceSongPurchasedQuality;
};

export type VoiceSongPurchasedDownloadCommand =
  | { kind: 'prepare' }
  | { kind: 'select'; quality: VoiceSongPurchasedQuality }
  | { kind: 'cancel'; selection: VoiceSongPurchasedSelection }
  | { kind: 'choose_playlist'; selection: VoiceSongPurchasedSelection; playlistId: string }
  | { kind: 'picker_status'; selection: VoiceSongPurchasedSelection; reconcile: boolean }
  | {
      kind: 'create_playlist';
      selection: VoiceSongPurchasedSelection;
      name: string;
      privatePlaylist: boolean;
    }
  | { kind: 'creation_status'; selection: VoiceSongPurchasedSelection; reconcile: boolean }
  | { kind: 'add_created_playlist'; selection: VoiceSongPurchasedSelection }
  | {
      kind: 'start';
      selection: VoiceSongPurchasedSelection;
      continuation: VoiceSongPurchasedContinuation | null;
      options: VoiceSongPurchasedDownloadOptions;
    };

export type VoiceSongPurchasedDownloadPicker = Omit<VoiceSongDownloadPicker, 'continuation'> & {
  continuation: VoiceSongPurchasedContinuation | null;
};

export type VoiceSongPurchasedDownloadCreation = Omit<VoiceSongDownloadCreation, 'add'> & {
  add: VoiceSongPurchasedDownloadPicker | null;
};

export type VoiceSongPurchasedDownloadResult =
  | { kind: 'plan'; plan: VoiceSongPurchasedDownloadPlan }
  | ({
      kind: 'selection';
      quality: VoiceSongPurchasedQuality;
    } & (
      | {
          decision: 'ready' | 'playlist_required';
          selection: VoiceSongPurchasedSelection;
        }
      | {
          decision: Exclude<VoiceSongDownloadDecision, 'ready' | 'playlist_required'>;
          selection: null;
        }
    ))
  | { kind: 'cancelled' }
  | { kind: 'picker'; picker: VoiceSongPurchasedDownloadPicker }
  | { kind: 'creation'; creation: VoiceSongPurchasedDownloadCreation }
  | { kind: 'directory_required' }
  | { kind: 'started'; result: DownloadResult };

export interface VoiceSongPurchasedDownloadClient {
  playerVoiceSongPurchasedDownload(
    target: VoiceSongRowTarget,
    command: VoiceSongPurchasedDownloadCommand,
  ): Promise<VoiceSongPurchasedDownloadResult>;
}

export interface VoiceSongLikedDestination {
  playlistId: string;
  destinationReceipt: string;
}

export interface VoiceSongLikedCollectClient {
  playerVoiceSongsCollectLikedDestination(
    target: VoiceSongCollectTarget,
  ): Promise<AccountData<VoiceSongLikedDestination | null>>;
  playerVoiceSongsCollectLiked(
    target: VoiceSongCollectTarget,
    destinationReceipt: string,
  ): Promise<VoiceSongCollectResult>;
}

declare const groupDownloadSelection: unique symbol;

/** Native permission for exactly one prepared group, held only in the active view. */
export type VoiceSongGroupDownloadReceipt = string & { readonly [groupDownloadSelection]: true };

export interface VoiceSongGroupDownloadCard {
  epoch: string;
  playId: string;
  cardReceipt: string;
}

export interface VoiceSongGroupDownloadRequest extends VoiceSongGroupDownloadCard {
  anchorIndex: number;
  indices: readonly number[];
}

export interface VoiceSongGroupDownloadTarget extends VoiceSongGroupDownloadCard {
  selectionReceipt: VoiceSongGroupDownloadReceipt;
}

type VoiceSongGroupDownloadBlocked =
  | 'playlist_required'
  | 'membership_unknown'
  | 'unavailable'
  | 'unsupported_selection'
  | 'directory_required';

export type VoiceSongGroupDownloadDecision = 'ready' | VoiceSongGroupDownloadBlocked;

export type VoiceSongGroupDownloadPlan = {
  epoch: string;
  playId: string;
  anchorIndex: number;
  eligibleIndices: readonly number[];
  omittedIndices: readonly number[];
} & (
  | { decision: 'ready'; selectionReceipt: VoiceSongGroupDownloadReceipt }
  | { decision: VoiceSongGroupDownloadBlocked; selectionReceipt: null }
);

export interface VoiceSongGroupDownloadClient {
  playerVoiceSongsDownloadPrepare(
    request: VoiceSongGroupDownloadRequest,
  ): Promise<VoiceSongGroupDownloadPlan>;
  playerVoiceSongsDownloadCancel(target: VoiceSongGroupDownloadTarget): Promise<void>;
  playerVoiceSongsDownloadStart(target: VoiceSongGroupDownloadTarget): Promise<DownloadResult>;
}

export interface VoiceSongsClient
  extends VoiceSongDownloadClient,
    VoiceSongPurchasedDownloadClient,
    VoiceSongGroupDownloadClient,
    VoiceSongCollectClient,
    VoiceSongLikedCollectClient {
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
