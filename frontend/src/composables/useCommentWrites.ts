import { computed, ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import { resourceKey, type CommentItem, type CommentResource } from '../services/comments.ts';
import {
  commentWritePending,
  commentWriteMessage,
  type CommentWrite,
  type CommentWriteAction,
  type CommentWriteClient,
  type CommentWriteCommand,
} from '../services/commentWrites.ts';
export function useCommentWrites(
  core: CommentWriteClient = useCore(),
  saved: (row: CommentWrite) => Promise<void> = async () => {},
  invalidate = () => {},
  expired = () => {},
) {
  const operations = shallowRef<CommentWrite[]>([]),
    busy = ref(false),
    loading = ref(false),
    error = ref(''),
    message = ref('');
  const draft = ref(''),
    reply = shallowRef<CommentItem>();
  const pending = computed(() => operations.value.filter(commentWritePending));
  const validText = computed(() => !!draft.value.trim() && draft.value.length <= 1000);
  let epoch = '',
    resource: CommentResource | undefined,
    generation = 0,
    reading = 0,
    connected = false;
  const live = (e: string, g: number) => connected && e === epoch && g === generation;
  function problem(value: unknown) {
    const p = failure(value);
    error.value = p.message;
    if (['session_expired', 'profile_changed'].includes(p.code)) {
      disconnect();
      expired();
    }
  }
  function disconnect() {
    generation++;
    reading++;
    connected = false;
    operations.value = [];
    draft.value = '';
    reply.value = undefined;
    busy.value = false;
    loading.value = false;
  }
  async function refresh() {
    if (!resource || !connected) return;
    const e = epoch,
      g = generation,
      r = resource,
      ticket = ++reading;
    loading.value = true;
    try {
      const result = await core.commentsWriteOperations(e, r);
      if (live(e, g) && ticket === reading && result.epoch === e)
        operations.value = result.value.filter((v) => resourceKey(v.resource) === resourceKey(r));
    } catch (value) {
      if (live(e, g) && ticket === reading) problem(value);
    } finally {
      if (live(e, g) && ticket === reading) loading.value = false;
    }
  }
  async function connect(nextEpoch: string, nextResource?: CommentResource) {
    disconnect();
    epoch = nextEpoch;
    resource = nextResource;
    error.value = '';
    message.value = '';
    connected = !!epoch && !!resource;
    await refresh();
  }
  function put(row: CommentWrite) {
    operations.value = [row, ...operations.value.filter((v) => v.id !== row.id)];
  }
  async function perform(
    command: CommentWriteCommand | null,
    row?: CommentWrite,
    action: CommentWriteAction = { kind: 'submit' },
  ) {
    if (!connected || !resource || busy.value || (command && pending.value.length)) return;
    if (
      row &&
      (!operations.value.some((v) => v.id === row.id) ||
        resourceKey(row.resource) !== resourceKey(resource))
    )
      return;
    const e = epoch,
      g = generation,
      selected = resource;
    busy.value = true;
    reading++;
    loading.value = false;
    error.value = '';
    message.value = '';
    invalidate();
    let id = row?.id;
    try {
      if (command) {
        const prepared = await core.commentsPrepareWrite(e, selected, command);
        if (
          !live(e, g) ||
          prepared.epoch !== e ||
          resourceKey(prepared.value.resource) !== resourceKey(selected)
        )
          return;
        put(prepared.value);
        id = prepared.value.id;
      }
      if (!id) return;
      const result = await core.commentsWriteCommand(e, id, action);
      if (
        !live(e, g) ||
        result.epoch !== e ||
        result.value.id !== id ||
        resourceKey(result.value.resource) !== resourceKey(selected)
      )
        return;
      reading++;
      put(result.value);
      message.value = commentWriteMessage(result.value);
      if (result.value.phase === 'confirmed') {
        if (
          result.value.command.kind === 'publish' &&
          draft.value === result.value.command.content &&
          (reply.value?.id ?? null) === result.value.command.replyTo
        ) {
          draft.value = '';
          reply.value = undefined;
        }
        await saved(result.value);
      }
      return result.value;
    } catch (value) {
      if (live(e, g)) {
        problem(value);
        if (live(e, g)) {
          await refresh();
          if (live(e, g)) {
            const unresolved = operations.value.find(
              (v) => v.id === id && commentWritePending(v) && v.phase !== 'prepared',
            );
            // A transport error may follow a successful public write. The
            // persisted responsibility takes precedence over generic retry text.
            if (unresolved) error.value = commentWriteMessage(unresolved);
          }
        }
      }
    } finally {
      if (live(e, g)) busy.value = false;
    }
  }
  function setReply(item?: CommentItem) {
    if (!busy.value) {
      reply.value = item;
      message.value = '';
    }
  }
  function reuse(row: CommentWrite) {
    if (
      busy.value ||
      pending.value.length ||
      !resource ||
      resourceKey(resource) !== resourceKey(row.resource) ||
      row.command.kind !== 'publish'
    )
      return;
    // If the target snapshot is unavailable, keep the identity instead of
    // silently turning a reply into a new top-level publication.
    if (row.command.replyTo && !row.target) {
      error.value = '请先定位原评论，再选择回复。';
      return;
    }
    draft.value = row.command.content;
    reply.value = row.command.replyTo ? (row.target ?? undefined) : undefined;
  }
  return {
    operations,
    pending,
    busy,
    loading,
    error,
    message,
    draft,
    reply,
    validText,
    connect,
    refresh,
    disconnect,
    setReply,
    reuse,
    publish: () =>
      validText.value
        ? perform({ kind: 'publish', content: draft.value, replyTo: reply.value?.id ?? null })
        : undefined,
    remove: (item: CommentItem) =>
      item.owned ? perform({ kind: 'delete', commentId: item.id }) : undefined,
    action: (row: CommentWrite, action: CommentWriteAction) => perform(null, row, action),
  };
}
export type CommentWritesModel = ReturnType<typeof useCommentWrites>;
