import { computed, ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type {
  ProfileFollowClient,
  ProfileFollowCommand,
  ProfileFollowView,
} from '../services/profileFollow';

const messages = {
  ready: '',
  followed: '关注成功',
  unfollowed: '取消关注成功',
  unchanged: '关注状态已刷新',
  uncertain: '操作结果尚未确认，请核对关注状态',
  limit: '关注已达上限',
  privacy: '根据对方设置，你没有该操作权限',
  blocked: '此用户当前不可关注',
  rejected: '操作未完成，请稍后重试',
};

export function useProfileFollow(
  core: ProfileFollowClient,
  initial: () => ProfileFollowView | null | undefined,
  expired: () => void,
  settle: (target: string, value: ProfileFollowView) => void,
) {
  const result = shallowRef<ProfileFollowView>();
  const active = computed(() => result.value ?? initial());
  const busy = ref(false);
  const issue = ref('');
  const message = computed(
    () => issue.value || (active.value ? messages[active.value.status] : ''),
  );
  let epoch = '';
  let uid = '';
  let visible = false;
  let revision = 0;

  function connect(owner: string, target: string | null) {
    visible = !!owner && !!target;
    // Detail/relations can sit above the same profile in history. Let an
    // already submitted result settle to its original cache while hidden.
    if (owner === epoch && (target === uid || target === null)) return;
    epoch = owner;
    uid = target ?? '';
    revision++;
    busy.value = false;
    result.value = undefined;
    issue.value = '';
  }

  function apply(value: ProfileFollowView) {
    result.value = value;
    settle(uid, value);
  }

  async function act(command: ProfileFollowCommand) {
    const shown = active.value;
    if (!visible || !epoch || !uid || busy.value || !shown) return;
    if (
      command.kind === 'change' &&
      (shown.pending || shown.relation?.followed !== command.expected)
    )
      return;
    if (command.kind === 'reconcile' && !shown.pending) return;
    const current = revision;
    busy.value = true;
    issue.value = '';
    try {
      const reply = await core.profileFollow(epoch, uid, command);
      if (revision !== current || reply.epoch !== epoch) return;
      apply(reply.value);
    } catch (error) {
      if (revision !== current) return;
      const code = failure(error).code;
      if (code === 'session_expired' || code === 'profile_changed') expired();
      else if (code === 'write_uncertain') {
        apply({ ...shown, pending: true, status: 'uncertain' });
      } else
        issue.value =
          code === 'busy' ? '此操作正在进行，请稍后核对' : '无法确认关注状态，请稍后重试';
    } finally {
      if (current === revision) busy.value = false;
    }
  }

  return { active, busy, message, connect, act };
}
