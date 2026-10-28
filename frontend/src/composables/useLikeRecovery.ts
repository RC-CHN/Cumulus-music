import { ref, shallowRef } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import type {
  LikeRecoveryClient,
  LikeRecoveryCommand,
  LikeRecoveryView,
} from '../services/likeRecovery.ts';
import { useCore } from '../services/core.ts';

export function useLikeRecovery(
  core: LikeRecoveryClient = useCore(),
  expired = () => {},
  changed = () => {},
) {
  const view = shallowRef<LikeRecoveryView>({ total: 0, offset: 0, operations: [] });
  const error = ref(''),
    notice = ref(''),
    busy = ref(false),
    open = ref(false);
  let epoch: string | undefined,
    serial = 0,
    disposed = false,
    dirty = false;
  const current = (owner: string, ticket: number) =>
    !disposed && epoch === owner && serial === ticket;
  function handle(value: unknown) {
    const issue = failure(value);
    error.value = issue.message;
    if (['session_expired', 'profile_changed'].includes(issue.code)) expired();
  }
  async function refresh(offset = view.value.offset) {
    if (!epoch || disposed) return;
    if (busy.value) {
      dirty = true;
      return;
    }
    const owner = epoch,
      ticket = ++serial;
    busy.value = true;
    error.value = '';
    notice.value = '';
    try {
      const result = await core.likeRecovery(owner, offset);
      if (current(owner, ticket) && result.epoch === owner) view.value = result.value;
    } catch (e) {
      if (current(owner, ticket)) handle(e);
    } finally {
      if (current(owner, ticket)) {
        busy.value = false;
        if (dirty) {
          dirty = false;
          void refresh();
        }
      }
    }
  }
  async function command(command: LikeRecoveryCommand) {
    if (!epoch || disposed || busy.value) return false;
    const owner = epoch,
      ticket = ++serial;
    busy.value = true;
    error.value = '';
    notice.value = '';
    let succeeded = false;
    try {
      const result = await core.recoverLike(owner, command);
      if (current(owner, ticket) && result.epoch === owner) {
        succeeded = true;
        notice.value =
          result.value.phase === 'confirmed'
            ? '已确认喜欢状态。'
            : result.value.phase === 'accepted_current_state'
              ? '已保留当前状态；原操作未被标为成功。'
              : '仍未达到原目标，可以稍后核对，或确认保留当前状态。';
        changed();
      }
    } catch (e) {
      if (current(owner, ticket)) handle(e);
    } finally {
      if (current(owner, ticket)) {
        // Lost IPC acknowledgements only inspect durable local state. Never
        // automatically repeat either the original write or acceptance.
        try {
          const result = await core.likeRecovery(owner, view.value.offset);
          if (current(owner, ticket) && result.epoch === owner) view.value = result.value;
        } catch (e) {
          if (current(owner, ticket)) handle(e);
        }
        if (current(owner, ticket)) {
          busy.value = false;
          if (dirty) {
            dirty = false;
            void refresh();
          }
        }
      }
    }
    return succeeded && current(owner, ticket);
  }
  function connect(account: AccountView) {
    const next = account.status === 'authenticated' ? account.epoch : undefined;
    if (next === epoch) return;
    epoch = next;
    serial++;
    dirty = false;
    busy.value = false;
    open.value = false;
    view.value = { total: 0, offset: 0, operations: [] };
    error.value = '';
    notice.value = '';
    if (next) void refresh(0);
  }
  function dispose() {
    disposed = true;
    serial++;
    open.value = false;
  }
  return { view, error, notice, busy, open, connect, refresh, command, dispose };
}
export type LikeRecoveryModel = ReturnType<typeof useLikeRecovery>;
