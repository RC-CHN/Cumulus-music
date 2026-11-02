import { ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import { resourceKey, type CommentResource } from '../services/comments.ts';
import {
  commentResourceId,
  type CommentResourceClient,
  type CommentResourceView,
} from '../services/commentResource.ts';

export function useCommentResource(core: CommentResourceClient, expired: () => void) {
  const view = shallowRef<CommentResourceView>(),
    loading = ref(false),
    error = ref('');
  let epoch = '',
    resource: CommentResource | undefined,
    serial = 0;
  function disconnect() {
    serial++;
    epoch = '';
    resource = undefined;
    view.value = undefined;
    loading.value = false;
    error.value = '';
  }
  async function read() {
    if (!epoch || !resource) return;
    const e = epoch,
      r = { ...resource },
      ticket = ++serial;
    const owns = () =>
      ticket === serial && epoch === e && resource && resourceKey(resource) === resourceKey(r);
    loading.value = true;
    error.value = '';
    const checked = (reply: { epoch: string; value: CommentResourceView }) => {
      if (
        reply.epoch !== e ||
        resourceKey(reply.value.resource) !== resourceKey(r) ||
        (reply.value.card &&
          (reply.value.card.kind !== r.kind || commentResourceId(reply.value.card) !== r.id))
      )
        throw { code: 'stale', message: '资源信息已变化，请重试' };
      return reply.value;
    };
    try {
      const next = checked(await core.commentsResource(e, r, true));
      if (owns()) view.value = next;
    } catch (value) {
      if (!owns()) return;
      const issue = failure(value);
      error.value = issue.message;
      if (['session_expired', 'profile_changed'].includes(issue.code)) {
        disconnect();
        expired();
        return;
      }
      try {
        const saved = checked(await core.commentsResource(e, r, false));
        if (owns()) view.value = saved;
      } catch {
        /* Keep the independently actionable metadata error and existing same-resource card. */
      }
    } finally {
      if (owns()) loading.value = false;
    }
  }
  function connect(e: string, r: CommentResource) {
    disconnect();
    epoch = e;
    resource = { ...r };
    return read();
  }
  return { view, loading, error, read, connect, disconnect };
}
