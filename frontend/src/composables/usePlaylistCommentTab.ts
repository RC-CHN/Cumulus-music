import { ref, watch } from 'vue';
import { useComments } from './useComments.ts';
import type { CommentClient } from '../services/comments.ts';
import type { AccountView } from '../services/contracts.ts';

export function usePlaylistCommentTab(
  client: CommentClient,
  account: () => AccountView,
  playlistId: () => string | null,
  expired: () => void,
) {
  const model = useComments(client, expired);
  const active = ref(false);
  const count = ref<number | null>(null);

  function hide() {
    if (!active.value) return;
    active.value = false;
    model.close();
  }

  watch(
    [() => account().epoch, () => account().status, playlistId],
    () => {
      hide();
      count.value = null;
      model.connect(account());
    },
    { immediate: true, flush: 'sync' },
  );
  watch(
    model.page,
    (page) => {
      if (active.value && page?.resource.kind === 'playlist' && page.resource.id === playlistId())
        count.value = page.total;
    },
    { flush: 'sync' },
  );

  function show() {
    const id = playlistId();
    if (active.value || !id || !model.epoch.value) return;
    active.value = true;
    return model.open({ kind: 'playlist', id });
  }

  return { model, active, count, show, hide };
}
