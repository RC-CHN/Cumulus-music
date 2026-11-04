import { nextTick, shallowRef, type InjectionKey } from 'vue';
import { failure } from '../services/contracts.ts';
import type {
  CommunityDeleteAction,
  CommunityDeleteClient,
  CommunityDeleteReply,
} from '../services/community.ts';

type Target = { detail: string; opener: HTMLElement | undefined };
export const communityDeleteKey: InjectionKey<ReturnType<typeof useCommunityDelete>> =
  Symbol('community-delete');

export function useCommunityDelete(
  core: CommunityDeleteClient,
  settled: (receipt: CommunityDeleteReply, detail: string) => void,
  expired: () => void,
) {
  const target = shallowRef<Target>();
  const status = shallowRef<CommunityDeleteReply['status']>();
  const pending = shallowRef(false);
  const error = shallowRef('');
  let epoch = '';
  let version = 0;
  let confirmation = '';

  function close(restore = true) {
    const opener = target.value?.opener;
    version++;
    target.value = undefined;
    status.value = undefined;
    pending.value = false;
    error.value = '';
    confirmation = '';
    if (restore && opener)
      void nextTick(() => {
        if (!target.value && opener.isConnected) opener.focus({ preventScroll: true });
      });
  }
  function connect(owner: string) {
    if (epoch === owner) return;
    epoch = owner;
    close(false);
  }
  async function read(action: CommunityDeleteAction) {
    const selected = target.value;
    if (!epoch || !selected || pending.value) return;
    const { kind } = action;
    const expected = version;
    pending.value = true;
    error.value = '';
    try {
      const reply = await core.communityNoteDelete(epoch, selected.detail, action);
      if (expected !== version || reply.epoch !== epoch) return;
      status.value = reply.value.status;
      confirmation = reply.value.confirmation ?? '';
      if (status.value === 'acknowledged' || status.value === 'observed_removed') {
        close(false);
        settled(reply.value, selected.detail);
      }
    } catch (cause) {
      if (expected !== version) return;
      const issue = failure(cause);
      if (kind === 'confirm') status.value = 'uncertain';
      error.value = kind === 'confirm' ? '无法确认删除结果。' : '暂时无法核对笔记。';
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
    } finally {
      if (expected === version) pending.value = false;
    }
  }
  function open(detail: string, opener: HTMLElement | undefined) {
    if (!epoch || target.value) return;
    target.value = { detail, opener };
    void read({ kind: 'inspect' });
  }
  const inspect = () => read({ kind: 'inspect' });
  const confirm = () =>
    status.value === 'ready' && confirmation && read({ kind: 'confirm', confirmation });
  const check = () =>
    status.value &&
    ['uncertain', 'still_visible', 'unavailable'].includes(status.value) &&
    read({ kind: 'check' });
  return { target, status, pending, error, connect, open, close, inspect, confirm, check };
}
