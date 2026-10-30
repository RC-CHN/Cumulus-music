import { ref, watch } from 'vue';
import type { CoreClient } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import { resourceKey } from '../services/comments.ts';
import type { CommentsModel } from './useComments.ts';
import { useCommentFloor } from './useCommentFloor.ts';
import { useCommentWrites } from './useCommentWrites.ts';

/** Comment interaction state can outlive a rendered album panel, never its account. */
export function useCommentContext(model: CommentsModel, core: CoreClient = useCore()) {
  const target = ref('');
  const floor = useCommentFloor(core, () => model.sessionExpired());
  const writer = useCommentWrites(
    core,
    async (row) => {
      const epoch = model.epoch.value,
        key = resourceKey(row.resource);
      if (row.command.kind === 'publish' && row.receiptId) await model.locate(row.receiptId);
      else await model.load();
      if (
        epoch === model.epoch.value &&
        model.resource.value &&
        resourceKey(model.resource.value) === key &&
        floor.query.value
      )
        await floor.load();
    },
    () => model.invalidate(),
    () => model.sessionExpired(),
  );
  const stop = watch(
    [() => model.epoch.value, () => model.resource.value && resourceKey(model.resource.value)],
    () => {
      target.value = '';
      floor.connect(model.epoch.value, model.resource.value);
      void writer.connect(model.epoch.value, model.resource.value);
    },
    { immediate: true, flush: 'sync' },
  );
  function dispose() {
    stop();
    target.value = '';
    writer.disconnect();
    floor.connect('');
  }
  return { target, floor, writer, dispose };
}
export type CommentContext = ReturnType<typeof useCommentContext>;
