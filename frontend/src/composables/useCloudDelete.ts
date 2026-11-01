import { computed, onScopeDispose, reactive, ref, shallowRef } from 'vue';
import type { CloudModel } from './useCloud.ts';
import type { CloudRemoval } from '../services/cloud.ts';
import type { CloudDeleteTarget } from '../services/cloudDelete.ts';
import { captureCloudDeleteTarget } from '../services/cloudDelete.ts';
export type { CloudDeleteTarget } from '../services/cloudDelete.ts';

export function useCloudDelete(model: CloudModel, input: CloudDeleteTarget, completed: () => void) {
  const original = captureCloudDeleteTarget(input);
  const target = reactive(captureCloudDeleteTarget(original));
  const removal = shallowRef<CloudRemoval>();
  const operation = computed(() => removal.value?.deletion ?? undefined);
  const unknown = ref(false),
    working = ref(false),
    preparing = ref(false),
    deleteLocal = ref(false);
  const activity = ref<'send' | 'read'>('send');
  const submitted = ref(false);
  let alive = true;
  const active = () => alive && model.accountEpoch.value === original.epoch;
  const stale = computed(() => model.view.value?.revision !== target.revision);
  const resumed = ref(false);
  const ready = () => active() && !working.value && !preparing.value && !model.busy.value;
  const valid = () =>
    original.ids.length > 0 &&
    original.ids.length <= 5000 &&
    original.ids.length === original.titles.length &&
    new Set(original.ids).size === original.ids.length;
  const canPrepare = computed(
    () =>
      ready() &&
      valid() &&
      (!stale.value || !!original.operationId) &&
      !removal.value &&
      !submitted.value,
  );
  const canSend = computed(
    () =>
      ready() &&
      !stale.value &&
      !!removal.value &&
      removal.value.phase === 'prepared' &&
      !removal.value.running &&
      !submitted.value &&
      !unknown.value,
  );
  const needsRead = computed(
    () => unknown.value || !!removal.value?.running || removal.value?.phase === 'remote',
  );
  const canRead = computed(() => ready() && !!removal.value && needsRead.value);
  const canContinue = computed(() => ready() && !unknown.value && !!removal.value?.canContinue);
  function accept(value: CloudRemoval, allowClose = false) {
    if (!active() || value.epoch !== original.epoch) return;
    removal.value = value;
    Object.assign(
      target,
      captureCloudDeleteTarget({
        epoch: value.epoch,
        ids: value.ids,
        titles: value.titles,
        kinds: value.kinds,
        revision: value.revision,
      }),
    );
    if (value.deleteLocal !== null) {
      deleteLocal.value = value.deleteLocal;
      submitted.value = true;
      unknown.value = false;
    }
    if (
      allowClose &&
      value.deletion?.phase === 'confirmed' &&
      !value.error &&
      !value.files.some((f) => ['retained', 'failed', 'removing'].includes(f.state))
    )
      completed();
  }
  async function prepare() {
    if (!canPrepare.value) return;
    preparing.value = true;
    try {
      const value = original.operationId
        ? await model.deleteRead(original.operationId)
        : await model.deletePrepare(original);
      if (value && !active()) {
        if (value.deleteLocal === null) void model.deleteDiscard(original.epoch, value.id);
      } else if (value) {
        resumed.value = value.deleteLocal !== null;
        accept(value);
      }
    } finally {
      if (active()) preparing.value = false;
    }
  }
  async function execute(continuing = false) {
    if (!(continuing ? canContinue.value : canSend.value) || !removal.value) return;
    working.value = true;
    activity.value = 'send';
    submitted.value = true;
    unknown.value = true;
    try {
      const value = await model.deleteExecute(removal.value.id, deleteLocal.value, continuing);
      if (value) accept(value, true);
    } finally {
      if (active()) working.value = false;
    }
  }
  async function read() {
    if (!canRead.value || !removal.value) return;
    working.value = true;
    activity.value = 'read';
    try {
      const value = await model.deleteRead(removal.value.id);
      if (value) accept(value, true);
    } finally {
      if (active()) working.value = false;
    }
  }
  onScopeDispose(() => {
    alive = false;
    if (removal.value && !submitted.value && removal.value.deleteLocal === null)
      void model.deleteDiscard(original.epoch, removal.value.id);
  });
  return {
    target,
    removal,
    operation,
    unknown,
    working,
    preparing,
    deleteLocal,
    resumed,
    submitted,
    activity,
    stale,
    canPrepare,
    canSend,
    canRead,
    needsRead,
    canContinue,
    prepare,
    send: () => execute(),
    resume: () => execute(true),
    read,
  };
}
