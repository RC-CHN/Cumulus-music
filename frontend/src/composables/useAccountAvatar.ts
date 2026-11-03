import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts.ts';
import {
  avatarCanAbandon,
  avatarContinue,
  avatarDraft,
  avatarTask,
  avatarTerminal,
  type AccountAvatarClient,
  type AvatarCommand,
  type AvatarTask,
} from '../services/accountAvatar.ts';
import type { CoverImage } from '../services/playlistCover.ts';

export function useAccountAvatar(
  core: AccountAvatarClient,
  owner: () => string | null,
  completed: (task: AvatarTask, observedNow: boolean) => void,
  expired: () => void,
) {
  const task = shallowRef<AvatarTask>();
  const image = shallowRef<CoverImage>();
  const error = ref('');
  const readFailed = ref(false);
  const open = ref(false);
  const phase = ref<
    | 'idle'
    | 'reading'
    | 'picking'
    | 'preview'
    | 'saving'
    | 'reconciling'
    | 'abandoning'
    | 'cleaning'
  >('idle');
  const busy = computed(() => phase.value !== 'idle');
  let disposed = false;
  let serial = 0;
  let reported: string | undefined;

  function clear() {
    serial++;
    open.value = false;
    phase.value = 'idle';
    error.value = '';
    readFailed.value = false;
    reported = undefined;
    task.value = undefined;
    image.value = undefined;
  }
  const stop = watch(owner, clear, { flush: 'sync' });
  onScopeDispose(() => {
    disposed = true;
    stop();
    clear();
  });
  function run<T>(
    state: typeof phase.value,
    work: (epoch: string, active: () => boolean) => Promise<T>,
  ) {
    const epoch = owner();
    if (!epoch || disposed || busy.value) return;
    const request = ++serial;
    const active = () => !disposed && request === serial && epoch === owner();
    phase.value = state;
    error.value = '';
    return work(epoch, active).finally(() => {
      if (active()) phase.value = 'idle';
    });
  }
  function checkEpoch(actual: string, expected: string) {
    if (actual !== expected) throw { code: 'avatar_owner_mismatch' };
  }
  function failed(cause: unknown, fallback: string) {
    const code = failure(cause).code;
    if (['stale', 'profile_changed', 'session_expired'].includes(code)) {
      clear();
      expired();
      return;
    }
    error.value = fallback;
  }
  function accept(next: AvatarTask | undefined) {
    if (task.value?.id !== next?.id || !avatarDraft(next)) image.value = undefined;
    task.value = next;
  }
  async function previewFor(epoch: string, active: () => boolean) {
    const id = task.value?.id;
    if (!id || !avatarDraft(task.value)) return;
    phase.value = 'preview';
    try {
      const response = await core.accountAvatarImage(epoch, id);
      if (!active() || task.value?.id !== id) return;
      checkEpoch(response.epoch, epoch);
      image.value = response.value;
    } catch (cause) {
      if (active()) failed(cause, '暂时无法读取所选图片，请重试。');
    }
  }
  async function chooseFor(epoch: string, active: () => boolean) {
    phase.value = 'picking';
    try {
      const response = await core.pickAccountAvatar(epoch);
      if (!active()) return false;
      checkEpoch(response.epoch, epoch);
      if (!response.value) return false;
      accept(response.value);
      return true;
    } catch (cause) {
      if (!active()) return false;
      const code = failure(cause).code;
      if (['invalid_input', 'cover_size', 'cover_dimensions', 'cover_format'].includes(code)) {
        failed(cause, '请选择 JPEG、PNG、BMP 或 GIF 图片，不超过 5 MiB，宽高至少 320 像素。');
      } else {
        readFailed.value = true;
        failed(cause, '暂时无法选取图片，请先重新读取头像编辑进度。');
      }
      return false;
    }
  }
  async function readFor(epoch: string, active: () => boolean, retainId?: string) {
    try {
      const result = await core.accountAvatarTasks(epoch);
      if (!active()) return false;
      checkEpoch(result.epoch, epoch);
      const previous = retainId && result.value.find(({ id }) => id === retainId);
      accept(avatarTask(result.value) ?? (previous || undefined));
      readFailed.value = false;
      return true;
    } catch (cause) {
      if (!active()) return false;
      readFailed.value = true;
      failed(cause, '暂时无法读取头像编辑进度，请重试。');
      return false;
    }
  }
  function start() {
    if (open.value) return;
    return run('reading', async (epoch, active) => {
      const loaded = await readFor(epoch, active);
      if (!active()) return;
      if (loaded && !task.value) await chooseFor(epoch, active);
      if (!active()) return;
      if (task.value || error.value || readFailed.value) open.value = true;
      if (task.value?.phase === 'confirmed' || task.value?.phase === 'accepted_current_state')
        onCompleted(task.value, false, false);
      else if (avatarDraft(task.value) && !image.value) await previewFor(epoch, active);
    });
  }
  function pick() {
    if (readFailed.value || (task.value && !avatarTerminal(task.value) && !avatarDraft(task.value)))
      return;
    return run('picking', async (epoch, active) => {
      if ((await chooseFor(epoch, active)) && active()) await previewFor(epoch, active);
    });
  }
  function preview() {
    if (readFailed.value) return;
    return run('preview', previewFor);
  }
  function reload() {
    return run('reading', async (epoch, active) => {
      const id = task.value?.id;
      if (!(await readFor(epoch, active, id)) || !active()) return;
      if (
        task.value &&
        task.value.id === id &&
        (task.value.phase === 'confirmed' || task.value.phase === 'accepted_current_state')
      )
        onCompleted(task.value, true, false);
      else if (avatarDraft(task.value) && !image.value) await previewFor(epoch, active);
    });
  }
  function onCompleted(next: AvatarTask, close = true, observedNow = true) {
    if (close && !next.cleanupPending && !next.metadataPending) open.value = false;
    if (reported !== next.id) {
      reported = next.id;
      completed(next, observedNow);
    }
  }
  function command(input: AvatarCommand) {
    const current = task.value;
    if (!current || readFailed.value) return;
    const allowed =
      input.kind === 'save'
        ? avatarDraft(current) && !!image.value
        : input.kind === 'continue'
          ? avatarContinue(current)
          : input.kind === 'reconcile'
            ? current.phase === 'binding'
            : input.kind === 'accept_current'
              ? current.phase === 'binding' && current.observedRevision === input.expectedRevision
              : input.kind === 'abandon'
                ? avatarCanAbandon(current)
                : current.cleanupPending || current.metadataPending;
    if (!allowed) return;
    const state =
      input.kind === 'reconcile' || input.kind === 'accept_current'
        ? 'reconciling'
        : input.kind === 'abandon'
          ? 'abandoning'
          : input.kind === 'cleanup'
            ? 'cleaning'
            : 'saving';
    return run(state, async (epoch, active) => {
      try {
        const response = await core.accountAvatarCommand(epoch, current.id, input);
        if (!active()) return;
        checkEpoch(response.epoch, epoch);
        if (response.value.id !== current.id) throw { code: 'avatar_task_mismatch' };
        accept(response.value);
        if (
          response.value.phase === 'confirmed' ||
          response.value.phase === 'accepted_current_state'
        )
          onCompleted(response.value, true, input.kind !== 'cleanup');
        else if (response.value.phase === 'abandoned' && !response.value.cleanupPending)
          open.value = false;
      } catch (cause) {
        if (!active()) return;
        readFailed.value = true;
        failed(cause, '操作回执未确认。请先读取头像编辑进度，再决定下一步。');
      }
    });
  }
  async function dismiss() {
    if (busy.value) return;
    if (avatarDraft(task.value) && !readFailed.value) {
      await command({ kind: 'abandon' });
      return;
    }
    open.value = false;
    image.value = undefined;
  }
  return {
    task,
    image,
    error,
    readFailed,
    open,
    phase,
    busy,
    start,
    pick,
    preview,
    reload,
    command,
    dismiss,
  };
}
export type AccountAvatarModel = ReturnType<typeof useAccountAvatar>;
