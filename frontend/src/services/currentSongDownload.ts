import type { PlaylistView } from './contracts.ts';
import type { DownloadOptions, DownloadResult } from './downloads.ts';
import type { PlayerView } from './player.ts';
import type { PlaylistPhase } from './playlists.ts';
import type {
  SearchGroupDownloadCommand,
  SearchGroupDownloadPlaylistResult,
  SearchGroupDownloadReply,
  SearchGroupDownloadSort,
  SearchPurchasedQuality,
} from './search.ts';

export interface CurrentSongDownloadTarget {
  playId: string;
  entryId: string;
}

export type CurrentSongDownloadCommand =
  | { kind: 'prepare' }
  | { kind: 'select'; offer: string; quality: SearchPurchasedQuality }
  | { kind: 'cancel'; receipt: string }
  | { kind: 'destinations'; selection: string; sort: SearchGroupDownloadSort; page: number }
  | { kind: 'choose_playlist'; selection: string; destination: string }
  | { kind: 'picker_status'; selection: string; reconcile: boolean }
  | { kind: 'create_playlist'; selection: string; name: string; private: boolean }
  | { kind: 'creation_status'; selection: string; reconcile: boolean }
  | { kind: 'add_created_playlist'; selection: string }
  | {
      kind: 'start';
      selection: string;
      continuation: string | null;
      options: DownloadOptions;
    };

type CurrentSongDownloadDecision =
  | 'ordinary'
  | 'unavailable'
  | 'purchased_quality_required'
  | 'ready'
  | 'playlist_required'
  | 'membership_unknown'
  | 'complete'
  | 'existing_task'
  | 'waiting'
  | 'working';

interface CurrentSongDownloadPicker {
  playlistId: string;
  operationId: string;
  phase: PlaylistPhase;
  addFailed: boolean;
  continuation: string | null;
}
interface CurrentSongDownloadCreation {
  createOperationId: string;
  createPhase: PlaylistPhase;
  playlistId: string | null;
  add: CurrentSongDownloadPicker | null;
  canResumeAdd: boolean;
}

export type CurrentSongDownloadReply = {
  epoch: string;
  playId: string;
  entryId: string;
  trackId: string | null;
} & (
  | {
      kind: 'plan';
      decision: CurrentSongDownloadDecision;
      offer?: string;
      purchasedQualities?: SearchPurchasedQuality[];
    }
  | {
      kind: 'selection';
      quality: SearchPurchasedQuality;
      decision: CurrentSongDownloadDecision;
      selection: string | null;
    }
  | { kind: 'cancelled' }
  | {
      kind: 'destinations';
      rows: { receipt: string; playlist: PlaylistView }[];
      nextPage: number | null;
      canCreate: boolean;
    }
  | { kind: 'picker'; picker: CurrentSongDownloadPicker }
  | { kind: 'creation'; creation: CurrentSongDownloadCreation }
  | { kind: 'directory_required' }
  | { kind: 'started'; result: DownloadResult }
);

export interface CurrentSongDownloadClient {
  playerCurrentSongDownload(
    epoch: string,
    target: CurrentSongDownloadTarget,
    command: CurrentSongDownloadCommand,
  ): Promise<CurrentSongDownloadReply>;
  openCurrentSongPurchasedQualityGuide(
    epoch: string,
    target: CurrentSongDownloadTarget,
    offer: string,
  ): Promise<void>;
}

export interface CurrentSongDownloadOwner {
  epoch: string;
  target: CurrentSongDownloadTarget;
  trackId: string;
}
export interface CurrentSongDownloadScope {
  owner: CurrentSongDownloadOwner;
  live: () => boolean;
  call: (command: CurrentSongDownloadCommand) => Promise<CurrentSongDownloadReply | undefined>;
  cancel: (receipt: string) => void;
  openGuide: (offer: string) => Promise<void>;
}

export function matchesCurrentSongDownload(
  owner: CurrentSongDownloadOwner,
  view: PlayerView | undefined,
): boolean {
  return (
    view?.epoch === owner.epoch &&
    view.playId === owner.target.playId &&
    view.entryId === owner.target.entryId &&
    view.resourceKind === 'catalog' &&
    view.track?.id === owner.trackId &&
    !view.track.local &&
    !view.track.missing
  );
}

export function currentSongDownloadScope(
  client: CurrentSongDownloadClient,
  owner: CurrentSongDownloadOwner,
  live: () => boolean,
): CurrentSongDownloadScope {
  const invoke = (command: CurrentSongDownloadCommand) =>
    client.playerCurrentSongDownload(owner.epoch, owner.target, command);
  const cancel = (receipt: string) => {
    void invoke({ kind: 'cancel', receipt }).catch(() => {});
  };
  return {
    owner,
    live,
    cancel,
    async openGuide(offer) {
      if (live() && offer)
        await client.openCurrentSongPurchasedQualityGuide(owner.epoch, owner.target, offer);
    },
    async call(command) {
      if (!live()) return;
      const reply = await invoke(command);
      if (
        reply.epoch !== owner.epoch ||
        reply.playId !== owner.target.playId ||
        reply.entryId !== owner.target.entryId ||
        (reply.trackId !== owner.trackId &&
          !(reply.kind === 'plan' && reply.decision === 'unavailable' && reply.trackId === null))
      )
        return;
      if (live()) return reply;
      if (reply.kind === 'plan' && reply.offer) cancel(reply.offer);
      if (reply.kind === 'selection' && reply.selection) cancel(reply.selection);
    },
  };
}

export interface CurrentSongDownloadPresentation {
  scope: CurrentSongDownloadScope;
  opener: HTMLElement;
  offer: string;
  choices: SearchPurchasedQuality[];
}

export function currentSongDownloadDecisionNotice(decision: CurrentSongDownloadDecision) {
  switch (decision) {
    case 'complete':
      return '该歌曲已下载或有匹配的本机文件。';
    case 'waiting':
      return '等待下载中';
    case 'working':
      return '现有下载任务正在处理。';
    case 'existing_task':
      return '已有下载任务，请到下载管理继续或修复。';
    case 'membership_unknown':
      return '暂未核实当前歌曲的喜欢状态，未添加下载任务。';
    default:
      return '暂未核实当前歌曲可用的下载选项。';
  }
}

export type DownloadPickerCommand =
  | Extract<
      SearchGroupDownloadCommand,
      {
        kind:
          | 'destinations'
          | 'choose'
          | 'choose_liked'
          | 'create_playlist'
          | 'creation_status'
          | 'add_created'
          | 'start_from_playlist';
      }
    >
  | { kind: 'cancel' | 'reconcile'; receipt: string };

export type DownloadPickerReply =
  | SearchGroupDownloadReply
  | { kind: 'scoped_accepted'; receipt: string; result: DownloadResult };
export type DownloadPickerRequest = (
  command: DownloadPickerCommand,
) => Promise<DownloadPickerReply | undefined>;

/** Only destination receipts returned for this exact native selection can be written. */
export function currentSongDownloadPicker(
  scope: CurrentSongDownloadScope,
  selection: string,
  options: DownloadOptions,
): DownloadPickerRequest {
  const destinations = new Map<string, string>();
  const epoch = scope.owner.epoch;
  let allowedContinuation: string | null = null;
  let startAttempted = false;
  const projectPicker = (picker: CurrentSongDownloadPicker): SearchGroupDownloadPlaylistResult => {
    allowedContinuation = picker.continuation;
    return {
      epoch,
      receipt: selection,
      playlistId: picker.playlistId,
      phase: picker.phase,
      addFailed: picker.addFailed,
      continuation: picker.continuation,
      likedIds: [],
    };
  };
  return async (command) => {
    if (command.receipt !== selection) return;
    switch (command.kind) {
      case 'cancel':
        scope.cancel(selection);
        return { kind: 'cancelled' };
      case 'destinations': {
        if (command.page === 0) destinations.clear();
        const reply = await scope.call({
          kind: 'destinations',
          selection,
          sort: command.sort,
          page: command.page,
        });
        if (reply?.kind !== 'destinations') return;
        const playlists: PlaylistView[] = [];
        for (const { receipt, playlist } of reply.rows) {
          if (!receipt || !playlist.owned || playlist.favorite || destinations.has(playlist.id))
            continue;
          destinations.set(playlist.id, receipt);
          playlists.push(playlist);
        }
        return {
          kind: 'destinations',
          page: { epoch, value: { playlists, nextPage: reply.nextPage } },
          systemLikedReceipt: null,
          canCreate: reply.canCreate,
        };
      }
      case 'choose':
      case 'reconcile': {
        const destination = command.kind === 'choose' ? destinations.get(command.playlistId) : null;
        if (command.kind === 'choose' && !destination) return;
        const reply = await scope.call(
          destination
            ? { kind: 'choose_playlist', selection, destination }
            : { kind: 'picker_status', selection, reconcile: true },
        );
        return reply?.kind === 'picker'
          ? { kind: 'playlist', result: projectPicker(reply.picker) }
          : undefined;
      }
      case 'choose_liked':
        return;
      case 'create_playlist':
      case 'creation_status':
      case 'add_created': {
        const reply = await scope.call(
          command.kind === 'create_playlist'
            ? {
                kind: 'create_playlist',
                selection,
                name: command.name,
                private: command.privatePlaylist,
              }
            : command.kind === 'creation_status'
              ? { kind: 'creation_status', selection, reconcile: command.reconcile }
              : { kind: 'add_created_playlist', selection },
        );
        if (reply?.kind !== 'creation') return;
        const { createPhase, playlistId, canResumeAdd, add } = reply.creation;
        return {
          kind: 'creation',
          result: {
            epoch,
            receipt: selection,
            createPhase,
            playlistId,
            canAdd: canResumeAdd,
            add: add ? projectPicker(add) : null,
          },
        };
      }
      case 'start_from_playlist': {
        if (startAttempted || !allowedContinuation || command.continuation !== allowedContinuation)
          return;
        startAttempted = true;
        const reply = await scope.call({
          kind: 'start',
          selection,
          continuation: command.continuation,
          options,
        });
        if (reply?.kind === 'directory_required') return { kind: 'directory_required' };
        if (reply?.kind !== 'started' || reply.result.epoch !== epoch) return;
        return { kind: 'scoped_accepted', receipt: selection, result: reply.result };
      }
    }
  };
}
