import { ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import type { CommunityNoticeCard, CommunityNoticeClient } from '../services/community.ts';

export function useCommunityNotice(core: CommunityNoticeClient, expired: () => void) {
  const dot = ref(false);
  const pending = ref(false);
  const card = shallowRef<CommunityNoticeCard | null>(null);
  const error = ref('');
  let account: string | null = null;
  let revision = 0;

  async function read(mode: 'badge' | 'visit') {
    const epoch = account;
    if (epoch === null || (mode === 'visit' && pending.value)) return;
    const request = ++revision;
    const visiting = mode === 'visit';
    if (visiting) {
      pending.value = true;
      error.value = '';
    }
    try {
      const response = await core.communityNewSongNotice(epoch, mode);
      if (request !== revision || response.epoch !== epoch) return;
      dot.value = response.value.showDot;
      if (visiting) card.value = response.value.card;
    } catch (reason) {
      if (request !== revision || !visiting) return;
      const issue = failure(reason);
      error.value = issue.message;
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
    } finally {
      if (request === revision && visiting) pending.value = false;
    }
  }

  function connect(epoch: string | null) {
    if (account === epoch) return;
    account = epoch;
    ++revision;
    dot.value = pending.value = false;
    card.value = null;
    error.value = '';
    if (epoch !== null) void read('badge');
  }

  return { dot, pending, card, error, connect, visit: () => read('visit') };
}
