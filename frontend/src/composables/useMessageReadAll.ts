import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue';
import { failure } from '../services/contracts';
import type { MessageClient, MessageReadAllAction, MessageReadAllView } from '../services/messages';

type Intent = { epoch: string; intent: string };

/** The actor owns unread counts and durable operations; the drawer owns only its visit. */
export function useMessageReadAll(
  core: Pick<MessageClient, 'messageReadAll'>,
  owner: () => string | undefined,
  confirmed: () => void,
  expired: () => void,
) {
  const confirmation = shallowRef<Intent>();
  const pending = shallowRef<{ operation: string; processing: boolean }>();
  const busy = ref(false);
  const notice = ref('');
  const statusFailed = ref(false);
  const blocked = computed(() => Boolean(pending.value || statusFailed.value));
  let visit = 0;

  function abandon(value?: Intent) {
    if (value)
      void core
        .messageReadAll(value.epoch, { kind: 'cancel', intent: value.intent })
        .catch(() => {});
  }

  function cancel() {
    if (busy.value) return;
    const value = confirmation.value;
    confirmation.value = undefined;
    abandon(value);
  }

  function reset() {
    visit++;
    abandon(confirmation.value);
    confirmation.value = undefined;
    pending.value = undefined;
    busy.value = false;
    notice.value = '';
    statusFailed.value = false;
  }

  function apply(epoch: string, kind: MessageReadAllAction['kind'], value: MessageReadAllView) {
    if (value.status === 'ready') {
      if (kind === 'prepare') confirmation.value = { epoch, intent: value.intent };
      return;
    }
    statusFailed.value = false;
    pending.value = undefined;
    if (kind === 'status') notice.value = '';
    switch (value.status) {
      case 'pending':
        pending.value = { operation: value.operation, processing: true };
        notice.value = '正在处理一键全读，可以稍后查看进度。';
        break;
      case 'uncertain':
      case 'check_unavailable':
      case 'check_inconclusive':
        pending.value = { operation: value.operation, processing: false };
        notice.value = {
          uncertain: '一键全读的结果尚未确认，可以核对未读数。',
          check_unavailable: '暂时无法核对未读数，仍保留待核对记录。',
          check_inconclusive: '当前仍无法确认清除结果，已保留待核对记录。',
        }[value.status];
        break;
      case 'confirmed':
        if (kind === 'confirm') {
          confirmed();
          notice.value = '已清除全部新消息提醒。';
        }
        break;
      case 'checked_clear':
        if (kind === 'check') notice.value = '当前没有未读消息，已结束核对。';
        break;
      case 'not_sent':
        if (kind === 'confirm') notice.value = '请求未发送，请刷新未读数后重试。';
        break;
      case 'unavailable':
        notice.value = '未读数暂不可用或已变化，请刷新后重试。';
        break;
    }
  }

  async function request(action: MessageReadAllAction) {
    const epoch = owner();
    if (!epoch || busy.value) return;
    const token = visit;
    const current = () => token === visit && epoch === owner();
    busy.value = true;
    if (action.kind === 'prepare') notice.value = '';
    try {
      const response = await core.messageReadAll(epoch, action);
      if (!current()) {
        if (response.epoch === epoch && response.value.status === 'ready')
          abandon({ epoch, intent: response.value.intent });
        return;
      }
      if (response.epoch !== epoch) throw new Error('message read-all epoch changed');
      apply(epoch, action.kind, response.value);
    } catch (error) {
      if (!current()) return;
      if (failure(error).code === 'session_expired') {
        expired();
        return;
      }
      if (action.kind === 'confirm' || action.kind === 'check') {
        // An IPC failure can occur after dispatch. Look up the local operation;
        // even a previously confirmed operation here is not this call's ACK.
        try {
          const status = await core.messageReadAll(epoch, { kind: 'status' });
          if (!current()) return;
          if (status.epoch !== epoch) throw error;
          if (status.value.status === 'pending' || status.value.status === 'uncertain') {
            apply(epoch, 'status', status.value);
            return;
          }
          pending.value = undefined;
        } catch {
          if (!current()) return;
          statusFailed.value = true;
          notice.value = '暂时无法读取一键全读状态，请查看进度。';
          return;
        }
      }
      if (action.kind === 'status') statusFailed.value = true;
      notice.value =
        action.kind === 'status'
          ? '暂时无法读取一键全读状态，请查看进度。'
          : '操作未获确认，请刷新未读数后再试。';
    } finally {
      if (current()) {
        if (action.kind === 'confirm') confirmation.value = undefined;
        busy.value = false;
      }
    }
  }

  function prepare() {
    if (!blocked.value && !confirmation.value) void request({ kind: 'prepare' });
  }
  function confirm() {
    const value = confirmation.value;
    if (value && value.epoch === owner()) void request({ kind: 'confirm', intent: value.intent });
  }
  function recover() {
    const value = pending.value;
    if (statusFailed.value || value?.processing) void request({ kind: 'status' });
    else if (value) void request({ kind: 'check', operation: value.operation });
  }

  watch(
    owner,
    (epoch) => {
      reset();
      if (epoch) void request({ kind: 'status' });
    },
    { immediate: true, flush: 'sync' },
  );
  onScopeDispose(reset);
  return {
    confirmation,
    busy,
    notice,
    blocked,
    canRecover: computed(() => Boolean(pending.value || statusFailed.value)),
    readProgress: computed(() => statusFailed.value || pending.value?.processing),
    prepare,
    confirm,
    cancel,
    recover,
  };
}
