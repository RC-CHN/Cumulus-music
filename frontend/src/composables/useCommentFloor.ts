import { ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import {
  resourceKey,
  type CommentResource,
  type CommentItem,
  type CommentReadNavigation,
  type CommentReadRetry,
} from '../services/comments.ts';
import type {
  CommentFloorClient,
  CommentFloorPage,
  CommentFloorQuery,
} from '../services/commentFloor.ts';
export function useCommentFloor(core: CommentFloorClient = useCore(), expired = () => {}) {
  const page = shallowRef<CommentFloorPage>(),
    query = shallowRef<CommentFloorQuery>(),
    history = shallowRef<CommentFloorQuery[]>([]),
    loading = ref(false),
    error = ref('');
  const retryTarget = shallowRef<CommentReadRetry<CommentFloorQuery>>();
  let epoch = '',
    resource: CommentResource | undefined,
    generation = 0,
    reading = 0;
  const live = (e: string, g: number, n: number) =>
    !!epoch && epoch === e && generation === g && reading === n;
  function close() {
    generation++;
    reading++;
    page.value = undefined;
    query.value = undefined;
    history.value = [];
    loading.value = false;
    error.value = '';
    retryTarget.value = undefined;
  }
  function connect(e: string, r?: CommentResource) {
    close();
    epoch = e;
    resource = r;
  }
  async function load(target = query.value, navigation: CommentReadNavigation = 'refresh') {
    if (!epoch || !resource || !target) return;
    const requested = { ...target };
    const e = epoch,
      g = generation,
      n = ++reading,
      r = resource;
    loading.value = true;
    error.value = '';
    retryTarget.value = undefined;
    const checked = (value: { epoch: string; value: CommentFloorPage }) => {
      const page = value.value;
      if (
        value.epoch !== e ||
        resourceKey(page.resource) !== resourceKey(r) ||
        page.query.parentId !== requested.parentId ||
        page.query.time !== requested.time ||
        page.query.limit !== requested.limit
      )
        throw { code: 'stale', message: '回复响应已变化，请重试' };
      return page;
    };
    let result: CommentFloorPage | undefined;
    try {
      result = checked(await core.commentsFloor(e, r, requested, true));
    } catch (value) {
      if (live(e, g, n)) {
        const p = failure(value);
        error.value = p.message;
        if (['session_expired', 'profile_changed'].includes(p.code)) {
          close();
          epoch = '';
          expired();
          return;
        }
        retryTarget.value = { query: requested, navigation };
        try {
          result = checked(await core.commentsFloor(e, r, requested, false));
        } catch {
          /* keep read error */
        }
      }
    } finally {
      if (live(e, g, n)) {
        loading.value = false;
        if (
          result &&
          resourceKey(result.resource) === resourceKey(r) &&
          result.query.parentId === requested.parentId &&
          result.query.time === requested.time
        ) {
          if (navigation === 'next' && query.value) history.value = [...history.value, query.value];
          if (navigation === 'back') history.value = history.value.slice(0, -1);
          if (navigation === 'reset') history.value = [];
          page.value = result;
          query.value = requested;
          if (error.value) retryTarget.value = { query: requested, navigation: 'refresh' };
        }
      }
    }
  }
  async function open(item: CommentItem) {
    close();
    query.value = { parentId: item.parentId ?? item.id, time: -1, limit: 20 };
    await load(query.value, 'reset');
  }
  return {
    page,
    query,
    history,
    loading,
    error,
    retryTarget,
    retry: () => {
      const target = retryTarget.value;
      if (!loading.value && target) return load(target.query, target.navigation);
    },
    connect,
    close,
    open,
    load,
    next: () => page.value?.next && load(page.value.next, 'next'),
    back: () => history.value.length && load(history.value.at(-1)!, 'back'),
  };
}
export type CommentFloorModel = ReturnType<typeof useCommentFloor>;
