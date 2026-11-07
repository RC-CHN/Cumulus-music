import { onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts.ts';
import type { DownloadOptions, DownloadResult, DownloadTab } from '../services/downloads.ts';
import { playlistLength, playlistPending, playlistSaved } from '../services/playlists.ts';
import type {
  VoiceSongDownloadClient,
  VoiceSongDownloadCreation,
  VoiceSongDownloadDecision,
  VoiceSongDownloadPicker,
  VoiceSongPurchasedContinuation,
  VoiceSongPurchasedDownloadClient,
  VoiceSongPurchasedDownloadCommand,
  VoiceSongPurchasedQuality,
  VoiceSongPurchasedSelection,
  VoiceSongRowTarget,
} from '../services/voiceSongs.ts';

export interface VoiceSongDownloadOpening {
  target: VoiceSongRowTarget;
  trackId: string;
  title: string;
}

type Recovery = { kind: 'existing'; playlistId: string } | { kind: 'creation' };
type Purchase = { selection: VoiceSongPurchasedSelection; quality: VoiceSongPurchasedQuality };
type DestinationCommand = Extract<
  VoiceSongPurchasedDownloadCommand,
  {
    kind:
      | 'choose_playlist'
      | 'picker_status'
      | 'create_playlist'
      | 'creation_status'
      | 'add_created_playlist';
  }
>;
type PurchasedFlow = {
  client: VoiceSongPurchasedDownloadClient;
  preferences: (epoch: string) => Promise<DownloadOptions>;
  started: (opening: VoiceSongDownloadOpening) => void;
  settings: (opening: VoiceSongDownloadOpening) => void;
};

type Stage =
  | { kind: 'idle' }
  | {
      kind: 'checking' | 'pick' | 'create' | 'starting' | 'directory';
      opening: VoiceSongDownloadOpening;
    }
  | {
      kind: 'quality';
      opening: VoiceSongDownloadOpening;
      choices: readonly VoiceSongPurchasedQuality[];
    }
  | {
      kind: 'recover';
      opening: VoiceSongDownloadOpening;
      recovery: Recovery;
      canResumeAdd: boolean;
    }
  | { kind: 'options'; opening: VoiceSongDownloadOpening; continuation?: string }
  | { kind: 'notice'; opening: VoiceSongDownloadOpening; downloadTab?: DownloadTab };

const notices: Record<Exclude<VoiceSongDownloadDecision, 'ready' | 'playlist_required'>, string> = {
  membership_unknown: '暂时无法确认喜欢状态。请先核对喜欢列表，再重新发起下载。',
  complete: '这首歌曲已下载。',
  existing_task: '下载管理中已有任务，可在那里继续。',
  waiting: '这首歌曲正在等待下载。',
  working: '这首歌曲正在下载。',
  unavailable: '这首歌曲暂不可下载。',
  extended_rights_unsupported: '这首歌曲需要独立的下载权益流程，暂不可从此处下载。',
  cloud_unsupported: '请到音乐云盘下载这首歌曲的原文件。',
};

export function useVoiceSongDownload(
  client: VoiceSongDownloadClient,
  ownsTarget: (target: VoiceSongRowTarget) => boolean,
  expired: () => void,
  purchased?: PurchasedFlow,
) {
  const stage = shallowRef<Stage>({ kind: 'idle' });
  const busy = ref(false);
  const message = ref('');
  let purchase: Purchase | undefined;

  function invalidate() {
    stage.value = { kind: 'idle' };
    busy.value = false;
    message.value = '';
    purchase = undefined;
  }
  function active(opening: VoiceSongDownloadOpening) {
    return (
      stage.value.kind !== 'idle' && stage.value.opening === opening && ownsTarget(opening.target)
    );
  }
  watch(
    () => stage.value.kind !== 'idle' && !ownsTarget(stage.value.opening.target),
    (retired) => {
      if (retired) invalidate();
    },
    { flush: 'sync' },
  );
  onScopeDispose(invalidate);

  function close() {
    const current = stage.value;
    if (busy.value && current.kind !== 'checking') return;
    if (current.kind !== 'idle' && purchase && active(current.opening))
      void purchased?.client
        .playerVoiceSongPurchasedDownload(current.opening.target, {
          kind: 'cancel',
          selection: purchase.selection,
        })
        .catch(() => {});
    invalidate();
  }
  function matches(
    opening: VoiceSongDownloadOpening,
    reply: { epoch: string; playId: string; trackId: string },
  ) {
    return (
      opening.target.epoch === reply.epoch &&
      opening.target.playId === reply.playId &&
      opening.trackId === reply.trackId
    );
  }
  function notice(opening: VoiceSongDownloadOpening, text: string, downloadTab?: DownloadTab) {
    message.value = text;
    stage.value = { kind: 'notice', opening, downloadTab };
  }
  function issue(opening: VoiceSongDownloadOpening, error: unknown) {
    if (!active(opening)) return;
    const result = failure(error);
    if (result.code === 'session_expired' || result.code === 'profile_changed') expired();
    if (active(opening)) return result.message;
  }
  async function request(value: VoiceSongDownloadOpening) {
    if (stage.value.kind !== 'idle' || !ownsTarget(value.target)) return;
    const opening = Object.freeze({ ...value, target: Object.freeze({ ...value.target }) });
    stage.value = { kind: 'checking', opening };
    busy.value = true;
    try {
      const plan = await client.playerVoiceSongDownloadPrepare(opening.target);
      if (!active(opening)) return;
      if (!matches(opening, plan)) return notice(opening, '无法核对当前歌曲的下载状态。');
      if (plan.decision === 'extended_rights_unsupported' && purchased) {
        const checked = await purchased.client.playerVoiceSongPurchasedDownload(opening.target, {
          kind: 'prepare',
        });
        if (!active(opening)) return;
        if (checked.kind !== 'plan' || !matches(opening, checked.plan))
          return notice(opening, '无法核对当前歌曲的下载状态。');
        if (
          checked.plan.decision === 'purchased_quality_required' &&
          checked.plan.purchasedQualities.length
        ) {
          stage.value = { kind: 'quality', opening, choices: [...checked.plan.purchasedQualities] };
          return;
        }
        if (
          checked.plan.decision !== 'ready' &&
          checked.plan.decision !== 'playlist_required' &&
          checked.plan.decision !== 'purchased_quality_required'
        )
          return showDecision(opening, checked.plan.decision);
        return notice(opening, '下载权限已变化，请重新选择歌曲。');
      }
      if (plan.decision === 'ready') stage.value = { kind: 'options', opening };
      else if (plan.decision === 'playlist_required') stage.value = { kind: 'pick', opening };
      else showDecision(opening, plan.decision);
    } catch (error) {
      const text = issue(opening, error);
      if (text) notice(opening, text);
    } finally {
      if (active(opening)) busy.value = false;
    }
  }
  function showDecision(opening: VoiceSongDownloadOpening, decision: keyof typeof notices) {
    notice(
      opening,
      notices[decision],
      decision === 'complete'
        ? 'completed'
        : ['existing_task', 'waiting', 'working'].includes(decision)
          ? 'active'
          : undefined,
    );
  }
  async function selectQuality(quality: VoiceSongPurchasedQuality) {
    const current = stage.value;
    if (
      !purchased ||
      current.kind !== 'quality' ||
      busy.value ||
      !current.choices.includes(quality) ||
      !active(current.opening)
    )
      return;
    const { opening } = current;
    busy.value = true;
    message.value = '';
    try {
      const reply = await purchased.client.playerVoiceSongPurchasedDownload(opening.target, {
        kind: 'select',
        quality,
      });
      if (!active(opening)) return;
      if (reply.kind !== 'selection' || reply.quality !== quality)
        return notice(opening, '无法核对此次音质选择。');
      if (reply.decision !== 'ready' && reply.decision !== 'playlist_required')
        return showDecision(opening, reply.decision);
      purchase = { selection: reply.selection, quality };
      if (reply.decision === 'playlist_required') stage.value = { kind: 'pick', opening };
      else await finishPurchased(opening, null);
    } catch (error) {
      const text = issue(opening, error);
      if (text) notice(opening, text);
    } finally {
      if (active(opening)) busy.value = false;
    }
  }
  async function finishPurchased(
    opening: VoiceSongDownloadOpening,
    continuation: VoiceSongPurchasedContinuation | null,
  ) {
    if (!purchased || !purchase || !active(opening)) return;
    const { selection, quality } = purchase;
    stage.value = { kind: 'starting', opening };
    try {
      const options = await purchased.preferences(opening.target.epoch);
      if (!active(opening)) return;
      const reply = await purchased.client.playerVoiceSongPurchasedDownload(opening.target, {
        kind: 'start',
        selection,
        continuation,
        options: { ...options, quality },
      });
      if (!active(opening)) return;
      if (reply.kind === 'directory_required') {
        stage.value = { kind: 'directory', opening };
        message.value = '当前下载目录不可用，请更改目录后重新发起下载。';
      } else if (reply.kind === 'started' && acceptStart(opening, reply.result)) {
        purchased.started(opening);
      } else if (reply.kind !== 'started') {
        notice(opening, '请到下载管理核对任务状态。', 'active');
      }
    } catch (error) {
      const text = issue(opening, error);
      if (text) notice(opening, text, 'active');
    }
  }
  function openDirectorySettings() {
    const current = stage.value;
    if (!purchased || current.kind !== 'directory' || busy.value || !active(current.opening))
      return;
    close();
    purchased.settings(current.opening);
  }
  function openCreate() {
    const current = stage.value;
    if (current.kind === 'pick' && !busy.value && active(current.opening)) {
      message.value = '';
      stage.value = { kind: 'create', opening: current.opening };
    }
  }
  function recover(
    opening: VoiceSongDownloadOpening,
    recovery: Recovery,
    text: string,
    canResumeAdd = false,
  ) {
    message.value = text;
    stage.value = { kind: 'recover', opening, recovery, canResumeAdd };
  }
  async function pickerResult(
    opening: VoiceSongDownloadOpening,
    recovery: Recovery,
    reply: VoiceSongDownloadPicker,
    continuation?: VoiceSongPurchasedContinuation,
  ) {
    if (
      !matches(opening, reply) ||
      (recovery.kind === 'existing' && reply.playlistId !== recovery.playlistId)
    ) {
      return recover(opening, recovery, '无法核对此次歌单操作。');
    }
    if (
      reply.continuation &&
      !playlistPending(reply.phase) &&
      (recovery.kind === 'existing' || (!reply.addFailed && playlistSaved(reply.phase)))
    ) {
      message.value = reply.addFailed ? '加入歌单失败；仍可继续设置下载。' : '';
      if (continuation && purchase) await finishPurchased(opening, continuation);
      else if (!purchase)
        stage.value = { kind: 'options', opening, continuation: reply.continuation };
      else notice(opening, '无法核对歌单下载许可，下载未开始。');
    } else if (playlistPending(reply.phase)) {
      recover(opening, recovery, '加入歌单的结果尚待确认。');
    } else notice(opening, '歌曲未能加入歌单，下载未开始。');
  }
  async function creationResult(
    opening: VoiceSongDownloadOpening,
    reply: VoiceSongDownloadCreation,
    continuation?: VoiceSongPurchasedContinuation,
  ) {
    const recovery = { kind: 'creation' } as const;
    if (!matches(opening, reply) || (reply.add && reply.add.playlistId !== reply.playlistId)) {
      return recover(opening, recovery, '无法核对此次新建歌单操作。');
    }
    if (reply.add && reply.playlistId && playlistSaved(reply.createPhase))
      return pickerResult(opening, recovery, reply.add, continuation);
    if (reply.playlistId && playlistSaved(reply.createPhase)) {
      return recover(opening, recovery, '歌单已创建；请确认是否将这首歌曲加入。', true);
    }
    if (playlistPending(reply.createPhase)) recover(opening, recovery, '创建歌单的结果尚待确认。');
    else notice(opening, '歌单未能创建，下载未开始。');
  }
  async function destinationAction<T>(
    opening: VoiceSongDownloadOpening,
    recovery: Recovery,
    action: () => Promise<T>,
    apply: (reply: T) => void | Promise<void>,
  ) {
    if (!active(opening) || busy.value) return;
    busy.value = true;
    message.value = '';
    try {
      const reply = await action();
      if (active(opening)) await apply(reply);
    } catch (error) {
      // A write reply may be lost. Only a subsequent explicit status read may
      // resolve it; never turn this error into a retry of the action.
      const text = issue(opening, error);
      if (text) recover(opening, recovery, text);
    } finally {
      if (active(opening)) busy.value = false;
    }
  }
  function purchaseDestination(
    opening: VoiceSongDownloadOpening,
    recovery: Recovery,
    command: DestinationCommand,
  ) {
    const client = purchased?.client;
    if (!client) return;
    return destinationAction(
      opening,
      recovery,
      () => client.playerVoiceSongPurchasedDownload(opening.target, command),
      (reply) => {
        if (recovery.kind === 'existing' && reply.kind === 'picker')
          return pickerResult(
            opening,
            recovery,
            reply.picker,
            reply.picker.continuation ?? undefined,
          );
        if (recovery.kind === 'creation' && reply.kind === 'creation')
          return creationResult(
            opening,
            reply.creation,
            reply.creation.add?.continuation ?? undefined,
          );
        recover(opening, recovery, '无法核对此次歌单操作。');
      },
    );
  }
  function selectPlaylist(playlistId: string) {
    const current = stage.value;
    if (current.kind !== 'pick' || !/^[1-9]\d*$/.test(playlistId)) return;
    const { opening } = current;
    const recovery = { kind: 'existing', playlistId } as const;
    if (purchase)
      return purchaseDestination(opening, recovery, {
        kind: 'choose_playlist',
        selection: purchase.selection,
        playlistId,
      });
    return destinationAction(
      opening,
      recovery,
      () => client.playerVoiceSongDownloadChoosePlaylist(opening.target, playlistId),
      (reply) => pickerResult(opening, recovery, reply),
    );
  }
  function submitCreate(name: string, privatePlaylist: boolean) {
    const current = stage.value;
    if (current.kind !== 'create' || busy.value) return;
    name = name.trim();
    if (
      !name ||
      /[@#]/.test(name) ||
      playlistLength(name) > 40 ||
      typeof privatePlaylist !== 'boolean'
    ) {
      message.value = '请输入有效的歌单名称。';
      return;
    }
    const { opening } = current;
    if (purchase)
      return purchaseDestination(
        opening,
        { kind: 'creation' },
        { kind: 'create_playlist', selection: purchase.selection, name, privatePlaylist },
      );
    return destinationAction(
      opening,
      { kind: 'creation' },
      () => client.playerVoiceSongDownloadCreatePlaylist(opening.target, name, privatePlaylist),
      (reply) => creationResult(opening, reply),
    );
  }
  function reconcile() {
    const current = stage.value;
    if (current.kind !== 'recover') return;
    const { opening, recovery } = current;
    if (purchase)
      return purchaseDestination(opening, recovery, {
        kind: recovery.kind === 'existing' ? 'picker_status' : 'creation_status',
        selection: purchase.selection,
        reconcile: true,
      });
    if (recovery.kind === 'existing')
      return destinationAction(
        opening,
        recovery,
        () => client.playerVoiceSongDownloadPickerStatus(opening.target, true),
        (reply) => pickerResult(opening, recovery, reply),
      );
    return destinationAction(
      opening,
      recovery,
      () => client.playerVoiceSongDownloadCreationStatus(opening.target, true),
      (reply) => creationResult(opening, reply),
    );
  }
  function resumeAdd() {
    const current = stage.value;
    if (current.kind !== 'recover' || current.recovery.kind !== 'creation' || !current.canResumeAdd)
      return;
    const { opening, recovery } = current;
    if (purchase)
      return purchaseDestination(opening, recovery, {
        kind: 'add_created_playlist',
        selection: purchase.selection,
      });
    return destinationAction(
      opening,
      recovery,
      () => client.playerVoiceSongDownloadAddCreatedPlaylist(opening.target),
      (reply) => creationResult(opening, reply),
    );
  }
  async function start(options: DownloadOptions) {
    const current = stage.value;
    if (current.kind !== 'options' || !active(current.opening) || busy.value) return false;
    const { opening, continuation } = current;
    busy.value = true;
    try {
      const result = continuation
        ? await client.playerVoiceSongDownloadStartFromPlaylist(
            opening.target,
            continuation,
            options,
          )
        : await client.playerVoiceSongDownloadStart(opening.target, options);
      if (!active(opening)) return false;
      return acceptStart(opening, result);
    } catch (error) {
      const text = issue(opening, error);
      if (text) notice(opening, text, 'active');
    } finally {
      if (active(opening)) busy.value = false;
    }
    return false;
  }
  function acceptStart(opening: VoiceSongDownloadOpening, result: DownloadResult) {
    if (
      result.epoch === opening.target.epoch &&
      result.accepted.length === 1 &&
      !result.failed.length
    ) {
      invalidate();
      return true;
    }
    notice(
      opening,
      (result.epoch === opening.target.epoch && result.failed[0]?.message) ||
        '请到下载管理核对任务状态。',
      'active',
    );
    return false;
  }
  return {
    stage,
    busy,
    message,
    request,
    close,
    selectQuality,
    openDirectorySettings,
    openCreate,
    selectPlaylist,
    submitCreate,
    reconcile,
    resumeAdd,
    start,
  };
}

export type VoiceSongDownloadModel = ReturnType<typeof useVoiceSongDownload>;
