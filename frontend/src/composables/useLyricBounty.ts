import { computed, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue';
import type { CoreClient } from '../services/contracts';
import type { LyricBountyAction, LyricBountyOpening } from '../services/player';

type Target = { epoch: string; playId: string };
type BountyClient = Pick<
  CoreClient,
  'lyricBountyActions' | 'lyricBountyPrepare' | 'lyricBountyConfirm' | 'lyricBountyCancel'
>;

export function useLyricBounty(core: BountyClient, target: () => Target | undefined) {
  const actions = shallowRef<LyricBountyAction[]>([]);
  const dialog = shallowRef<LyricBountyOpening & { action: LyricBountyAction }>();
  const preparing = ref(false);
  const sending = ref(false);
  const online = ref(navigator.onLine === true);
  const problem = ref(false);
  const canConfirm = computed(() => !!dialog.value?.id && online.value && !sending.value);
  let generation = 0;
  let owner: HTMLElement | null = null;

  function release(id: string | null | undefined) {
    if (id) void core.lyricBountyCancel(id).catch(() => {});
  }
  function retire() {
    generation++;
    release(dialog.value?.id);
    dialog.value = undefined;
    preparing.value = false;
    sending.value = false;
    problem.value = false;
  }
  function syncOnline() {
    online.value = navigator.onLine === true;
    const open = dialog.value;
    if (online.value || !open?.id || sending.value) return;
    release(open.id);
    dialog.value = { ...open, id: null, status: 'not_sent' };
  }
  watch(
    target,
    (next) => {
      retire();
      actions.value = [];
      if (!next) return;
      const current = generation;
      void core.lyricBountyActions(next.epoch, next.playId).then(
        (result) => {
          if (current === generation) actions.value = result;
        },
        () => {},
      );
    },
    { immediate: true, flush: 'sync' },
  );
  onMounted(() => {
    window.addEventListener('online', syncOnline);
    window.addEventListener('offline', syncOnline);
  });
  onUnmounted(() => {
    window.removeEventListener('online', syncOnline);
    window.removeEventListener('offline', syncOnline);
    retire();
  });

  async function open(action: LyricBountyAction, opener: EventTarget | null) {
    const selection = target();
    if (!selection || preparing.value || dialog.value || !actions.value.includes(action)) return;
    owner = opener instanceof HTMLElement ? opener : null;
    const current = generation;
    preparing.value = true;
    problem.value = false;
    try {
      const opening = await core.lyricBountyPrepare(selection.epoch, selection.playId, action);
      if (current !== generation) return release(opening.id);
      dialog.value = { action, ...opening };
      syncOnline();
    } catch {
      if (current === generation) problem.value = true;
    } finally {
      if (current === generation) preparing.value = false;
    }
  }
  function close() {
    if (sending.value) return;
    release(dialog.value?.id);
    dialog.value = undefined;
  }
  async function confirm() {
    syncOnline();
    const open = dialog.value;
    if (!open?.id || !canConfirm.value) return;
    const current = generation;
    sending.value = true;
    dialog.value = { ...open, id: null };
    const status = await core.lyricBountyConfirm(open.id).catch(() => 'unknown' as const);
    if (current !== generation) return;
    dialog.value = { action: open.action, id: null, status };
    sending.value = false;
  }
  return {
    actions,
    dialog,
    preparing,
    sending,
    canConfirm,
    problem,
    opener: () => owner,
    open,
    close,
    confirm,
  };
}
