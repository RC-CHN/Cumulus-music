import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue';
import type { MemberCardOffer, MemberPortalClient } from '../services/member.ts';
import { failure } from '../services/contracts.ts';

/** One visible account's optional delivery slot; a failed slot never blocks the page. */
export function useMemberOffer(core: MemberPortalClient, owner: () => string | undefined) {
  const observed = shallowRef<{ epoch: string; value: MemberCardOffer }>();
  const offer = computed(() =>
    observed.value?.epoch === owner() ? observed.value?.value : undefined,
  );
  const issue = ref('');
  const loading = ref(false);
  const opening = ref(false);
  let serial = 0;
  let disposed = false;

  function clear() {
    serial++;
    observed.value = undefined;
    issue.value = '';
    loading.value = false;
    opening.value = false;
  }

  async function load(refresh = false) {
    const epoch = owner();
    if (!epoch || loading.value || opening.value || disposed) return;
    const ticket = ++serial;
    const active = () => !disposed && owner() === epoch && ticket === serial;
    loading.value = true;
    issue.value = '';
    if (refresh) observed.value = undefined;
    try {
      const response = await core.memberCardOffer(epoch, refresh);
      if (active() && response.epoch === epoch) observed.value = response;
    } catch {
      if (active()) observed.value = undefined;
    } finally {
      if (active()) loading.value = false;
    }
  }

  async function openBrowser() {
    const epoch = owner();
    const entry = offer.value;
    if (!epoch || entry?.mode !== 'browser' || opening.value || disposed) return;
    const ticket = ++serial;
    const active = () => !disposed && owner() === epoch && ticket === serial;
    opening.value = true;
    issue.value = '';
    try {
      await core.openMemberCardOffer(epoch, entry.revision);
    } catch (error) {
      if (active()) {
        const expired = ['stale', 'session_expired', 'profile_changed'].includes(
          failure(error).code,
        );
        if (expired) observed.value = undefined;
        issue.value = expired ? '优惠入口已更新，请重新读取。' : '暂时无法打开浏览器。';
      }
    } finally {
      if (active()) opening.value = false;
    }
  }

  watch(
    owner,
    () => {
      clear();
      void load();
    },
    { immediate: true },
  );
  onScopeDispose(() => {
    disposed = true;
    clear();
  });
  return { offer, issue, opening, load, openBrowser };
}
