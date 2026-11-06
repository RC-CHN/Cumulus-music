import { onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts.ts';
import type { DownloadOptions, DownloadTab } from '../services/downloads.ts';
import { playlistLength, playlistPending, playlistSaved } from '../services/playlists.ts';
import type {
  VoiceSongDownloadClient,
  VoiceSongDownloadCreation,
  VoiceSongDownloadDecision,
  VoiceSongDownloadPicker,
  VoiceSongRowTarget,
} from '../services/voiceSongs.ts';

export interface VoiceSongDownloadOpening {
  target: VoiceSongRowTarget;
  trackId: string;
  title: string;
}

type Recovery = { kind: 'existing'; playlistId: string } | { kind: 'creation' };

type Stage =
  | { kind: 'idle' }
  | { kind: 'checking' | 'pick' | 'create'; opening: VoiceSongDownloadOpening }
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
) {
  const stage = shallowRef<Stage>({ kind: 'idle' });
  const busy = ref(false);
  const message = ref('');

  function invalidate() {
    stage.value = { kind: 'idle' };
    busy.value = false;
    message.value = '';
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
    if (!busy.value || stage.value.kind === 'checking') invalidate();
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
      if (plan.decision === 'ready') stage.value = { kind: 'options', opening };
      else if (plan.decision === 'playlist_required') stage.value = { kind: 'pick', opening };
      else
        notice(
          opening,
          notices[plan.decision],
          plan.decision === 'complete'
            ? 'completed'
            : ['existing_task', 'waiting', 'working'].includes(plan.decision)
              ? 'active'
              : undefined,
        );
    } catch (error) {
      const text = issue(opening, error);
      if (text) notice(opening, text);
    } finally {
      if (active(opening)) busy.value = false;
    }
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
  function pickerResult(
    opening: VoiceSongDownloadOpening,
    recovery: Recovery,
    reply: VoiceSongDownloadPicker,
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
      stage.value = { kind: 'options', opening, continuation: reply.continuation };
    } else if (playlistPending(reply.phase)) {
      recover(opening, recovery, '加入歌单的结果尚待确认。');
    } else notice(opening, '歌曲未能加入歌单，下载未开始。');
  }
  function creationResult(opening: VoiceSongDownloadOpening, reply: VoiceSongDownloadCreation) {
    const recovery = { kind: 'creation' } as const;
    if (!matches(opening, reply) || (reply.add && reply.add.playlistId !== reply.playlistId)) {
      return recover(opening, recovery, '无法核对此次新建歌单操作。');
    }
    if (reply.add && reply.playlistId && playlistSaved(reply.createPhase))
      return pickerResult(opening, recovery, reply.add);
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
    apply: (reply: T) => void,
  ) {
    if (!active(opening) || busy.value) return;
    busy.value = true;
    message.value = '';
    try {
      const reply = await action();
      if (active(opening)) apply(reply);
    } catch (error) {
      // A write reply may be lost. Only a subsequent explicit status read may
      // resolve it; never turn this error into a retry of the action.
      const text = issue(opening, error);
      if (text) recover(opening, recovery, text);
    } finally {
      if (active(opening)) busy.value = false;
    }
  }
  function selectPlaylist(playlistId: string) {
    const current = stage.value;
    if (current.kind !== 'pick' || !/^[1-9]\d*$/.test(playlistId)) return;
    const { opening } = current;
    const recovery = { kind: 'existing', playlistId } as const;
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
    } catch (error) {
      const text = issue(opening, error);
      if (text) notice(opening, text, 'active');
    } finally {
      if (active(opening)) busy.value = false;
    }
    return false;
  }
  return {
    stage,
    busy,
    message,
    request,
    close,
    openCreate,
    selectPlaylist,
    submitCreate,
    reconcile,
    resumeAdd,
    start,
  };
}

export type VoiceSongDownloadModel = ReturnType<typeof useVoiceSongDownload>;
