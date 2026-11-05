import { onScopeDispose, shallowRef, watch, type Ref } from 'vue';
import { resourceKey, type CommentItem, type CommentResource } from '../services/comments.ts';
import type {
  CommentWriteClient,
  CommentWriteCommand,
  VinylCommentWriteClient,
} from '../services/commentWrites.ts';
import { useCommentWrites, type CommentWritesModel } from './useCommentWrites.ts';

export interface VinylComposerTarget {
  epoch: string;
  playId: string;
  resource: CommentResource & { kind: 'track' };
  title: string;
}

export interface VinylComposerOpening {
  id: number;
  target: VinylComposerTarget;
  reply?: CommentItem;
  opener?: HTMLElement;
  writer: CommentWritesModel;
}

function targetKey(target?: VinylComposerTarget) {
  return target && JSON.stringify([target.epoch, target.playId, resourceKey(target.resource)]);
}

export function useVinylCommentComposer(
  core: VinylCommentWriteClient,
  target: Readonly<Ref<VinylComposerTarget | undefined>>,
  confirmed: (target: VinylComposerTarget, item: CommentItem) => void,
) {
  const current = shallowRef<VinylComposerOpening>();
  let sequence = 0;
  function close(expected = current.value) {
    if (!expected || current.value !== expected) return;
    current.value = undefined;
    expected.writer.disconnect();
  }
  const live = (opening: VinylComposerOpening) =>
    current.value === opening && targetKey(opening.target) === targetKey(target.value);
  watch(
    () => targetKey(target.value),
    () => close(),
    { flush: 'sync' },
  );
  onScopeDispose(close);

  function open(expected: VinylComposerTarget, reply?: CommentItem, opener?: HTMLElement) {
    if (current.value || targetKey(expected) !== targetKey(target.value)) return;
    const selected = { ...expected, resource: { ...expected.resource } };
    const recipient = reply ? { ...reply, author: reply.author && { ...reply.author } } : undefined;
    let opening: VinylComposerOpening;
    let prepared:
      | { id: string; command: Extract<CommentWriteCommand, { kind: 'publish' }> }
      | undefined;
    let submitted = false;
    const check = (epoch: string, resource?: CommentResource) => {
      if (
        !live(opening) ||
        epoch !== selected.epoch ||
        (resource && resourceKey(resource) !== resourceKey(selected.resource))
      )
        throw new Error('评论对象已变化');
    };
    const scoped: CommentWriteClient = {
      async commentsWriteOperations(epoch, resource) {
        check(epoch, resource);
        return core.playerVinylWriteOperations(epoch, selected.playId);
      },
      async commentsPrepareWrite(epoch, resource, command) {
        check(epoch, resource);
        if (command.kind !== 'publish' || command.replyTo !== (recipient?.id ?? null) || prepared)
          throw new Error('回复对象已变化');
        const result = await core.playerVinylPrepareWrite(epoch, selected.playId, command);
        check(epoch, result.value.resource);
        const saved = result.value.command;
        if (
          result.epoch !== epoch ||
          result.value.phase !== 'prepared' ||
          saved.kind !== 'publish' ||
          saved.content !== command.content ||
          saved.replyTo !== command.replyTo
        )
          throw new Error('评论操作未得到确认');
        prepared = { id: result.value.id, command: { ...saved } };
        return result;
      },
      async commentsWriteCommand(epoch, id, action) {
        check(epoch);
        if (action.kind !== 'submit' || !prepared || id !== prepared.id || submitted)
          throw new Error('请在独立评论页核对这次操作');
        submitted = true;
        const result = await core.playerVinylWriteCommand(epoch, selected.playId, id, {
          kind: 'submit',
        });
        check(epoch, result.value.resource);
        const returned = result.value.command;
        if (
          result.epoch !== epoch ||
          result.value.id !== id ||
          returned.kind !== 'publish' ||
          returned.replyTo !== prepared.command.replyTo ||
          returned.content !== prepared.command.content
        )
          throw new Error('评论操作回执不匹配');
        if (result.value.phase === 'rejected') {
          // A definitive rejection permits a new explicit intent, never a replay.
          prepared = undefined;
          submitted = false;
        }
        return result;
      },
    };
    const writer = useCommentWrites(scoped, async (row) => {
      if (
        !live(opening) ||
        row.id !== prepared?.id ||
        row.command.kind !== 'publish' ||
        row.command.replyTo !== (recipient?.id ?? null) ||
        !row.observed ||
        row.observed.id !== row.receiptId
      )
        return;
      confirmed(selected, row.observed);
      close(opening);
    });
    opening = { id: ++sequence, target: selected, reply: recipient, opener, writer };
    current.value = opening;
    void writer.connect(selected.epoch, selected.resource);
    writer.setReply(recipient);
  }

  return { current, open, close, live };
}
export type VinylCommentComposer = ReturnType<typeof useVinylCommentComposer>;
