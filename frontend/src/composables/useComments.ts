import { ref, shallowRef } from 'vue';
import type { AccountView } from '../services/contracts.ts';
import { failure } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import {
  commentQuery,
  resourceKey,
  type CommentClient,
  type CommentItem,
  type CommentOperation,
  type CommentPage,
  type CommentQuery,
  type CommentResource,
} from '../services/comments.ts';
export function useComments(core: CommentClient = useCore(), expired: () => void = () => {}) {
  const epoch = ref(''),
    resource = shallowRef<CommentResource>(),
    page = shallowRef<CommentPage>(),
    query = shallowRef(commentQuery());
  const loading = ref(false),
    error = ref(''),
    actionError = ref(''),
    busy = shallowRef(new Set<string>()),
    history = shallowRef<CommentQuery[]>([]);
  let generation = 0,
    listing = 0,
    writes = 0;
  const valid = (g: number, e: string) => g === generation && e === epoch.value && !!e;
  function issue(value: unknown, action = false) {
    const e = failure(value);
    (action ? actionError : error).value = e.message;
    if (e.code === 'session_expired' || e.code === 'profile_changed') sessionExpired();
  }
  function sessionExpired() {
    generation++;
    listing++;
    epoch.value = '';
    resource.value = undefined;
    page.value = undefined;
    loading.value = false;
    busy.value = new Set();
    history.value = [];
    expired();
  }
  function connect(account: AccountView) {
    const next = account.status === 'authenticated' ? account.epoch : '';
    if (next === epoch.value) return;
    generation++;
    listing++;
    epoch.value = next;
    resource.value = undefined;
    page.value = undefined;
    loading.value = false;
    busy.value = new Set();
    history.value = [];
    error.value = '';
    actionError.value = '';
  }
  async function load(
    target = query.value,
    navigation: 'refresh' | 'next' | 'back' | 'reset' = 'refresh',
  ) {
    const selected = resource.value,
      e = epoch.value,
      g = generation,
      ticket = ++listing,
      revision = writes;
    if (!selected || !e) return;
    loading.value = true;
    error.value = '';
    let result: CommentPage | undefined;
    try {
      const reply = await core.commentsPage(e, selected, target, true);
      if (reply.epoch === e) result = reply.value;
    } catch (value) {
      if (valid(g, e) && ticket === listing) {
        issue(value);
        if (!valid(g, e)) return;
        try {
          const reply = await core.commentsPage(e, selected, target, false);
          if (reply.epoch === e) result = reply.value;
        } catch {
          /* Keep the actionable network error. */
        }
      }
    } finally {
      if (valid(g, e) && ticket === listing) {
        loading.value = false;
        // A page requested before a write must not overwrite its confirmed flag.
        if (
          revision === writes &&
          result &&
          resourceKey(result.resource) === resourceKey(selected)
        ) {
          if (navigation === 'next') history.value = [...history.value, query.value];
          if (navigation === 'back') history.value = history.value.slice(0, -1);
          if (navigation === 'reset') history.value = [];
          page.value = result;
          query.value = target;
        }
      }
    }
  }
  async function open(next: CommentResource) {
    generation++;
    listing++;
    resource.value = next;
    page.value = undefined;
    query.value = commentQuery();
    history.value = [];
    busy.value = new Set();
    error.value = '';
    actionError.value = '';
    await load();
  }
  function apply(result: CommentOperation) {
    if (!page.value) return;
    const p = page.value,
      item = result.observed;
    const update = (row: CommentItem) => (item && row.id === item.id ? item : row);
    page.value = {
      ...p,
      comments: p.comments.map(update),
      hotComments: p.hotComments.map(update),
      topComments: p.topComments.map(update),
      currentComment: p.currentComment ? update(p.currentComment) : null,
      operations: p.operations.filter((o) => o.id !== result.id),
    };
  }
  async function mutate(id: string, desired: boolean | null) {
    const selected = resource.value,
      e = epoch.value,
      g = generation;
    if (
      !selected ||
      !e ||
      busy.value.has(id) ||
      (desired !== null && page.value?.operations.some((o) => o.id === id))
    )
      return;
    busy.value = new Set(busy.value).add(id);
    writes++;
    actionError.value = '';
    try {
      const reply =
        desired === null
          ? await core.commentsReconcile(e, selected, id)
          : await core.commentsLike(e, selected, id, desired);
      if (valid(g, e) && reply.epoch === e) {
        writes++;
        apply(reply.value);
      }
    } catch (value) {
      if (valid(g, e)) {
        issue(value, true);
        if (!valid(g, e)) return;
        try {
          const reply = await core.commentsPage(e, selected, query.value, false);
          if (valid(g, e) && reply.epoch === e && page.value)
            page.value = { ...page.value, operations: reply.value.operations };
        } catch {
          /* Preserve last view and the write error. */
        }
      }
    } finally {
      if (valid(g, e)) {
        const next = new Set(busy.value);
        next.delete(id);
        busy.value = next;
      }
    }
  }
  function locate(id: string) {
    if (!/^[0-9]+$/.test(id) || !/[1-9]/.test(id)) {
      error.value = '请输入有效的评论编号';
      return;
    }
    return load({ ...commentQuery(), commentId: id }, 'reset');
  }
  return {
    epoch,
    resource,
    page,
    query,
    loading,
    error,
    actionError,
    busy,
    history,
    connect,
    open,
    load,
    locate,
    invalidate: () => {
      writes++;
      listing++;
      loading.value = false;
    },
    sessionExpired,
    sort: (value: 'latest' | 'hot') => load(commentQuery(value), 'reset'),
    next: () => page.value?.next && load(page.value.next, 'next'),
    back: () => history.value.length && load(history.value.at(-1)!, 'back'),
    like: (item: CommentItem) => item.liked !== null && mutate(item.id, !item.liked),
    reconcile: (id: string) => mutate(id, null),
  };
}
export type CommentsModel = ReturnType<typeof useComments>;
