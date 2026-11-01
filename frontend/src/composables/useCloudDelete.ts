import { computed, onScopeDispose, ref, shallowRef } from 'vue';
import type { CloudModel } from './useCloud.ts';
import type { CloudDelete } from '../services/cloud.ts';
import type { CloudDeleteTarget } from '../services/cloudDelete.ts';
import { captureCloudDeleteTarget } from '../services/cloudDelete.ts';
export type { CloudDeleteTarget } from '../services/cloudDelete.ts';

export function useCloudDelete(model: CloudModel, input: CloudDeleteTarget, completed: () => void) {
  const target = captureCloudDeleteTarget(input);
  const operation = shallowRef<CloudDelete>();
  const unknown = ref(false),
    working = ref(false);
  const activity = ref<'send' | 'read'>('send');
  const known = new Set(model.view.value?.deletions.map((v) => v.id));
  let alive = true;
  onScopeDispose(() => (alive = false));
  const active = () => alive && model.accountEpoch.value === target.epoch;
  const pending = (v: CloudDelete) => ['sending', 'verifying', 'uncertain'].includes(v.phase);
  const related = (v: CloudDelete) => v.ids.some((id) => target.ids.includes(id));
  operation.value = model.view.value?.deletions.find((v) => pending(v) && related(v));
  const stale = computed(() => model.view.value?.revision !== target.revision);
  const canSend = computed(
    () =>
      active() &&
      !stale.value &&
      target.ids.length > 0 &&
      target.ids.length <= 5000 &&
      target.ids.length === target.titles.length &&
      new Set(target.ids).size === target.ids.length &&
      !operation.value &&
      !unknown.value &&
      !working.value &&
      !model.busy.value,
  );
  const canRead = computed(
    () =>
      active() &&
      !working.value &&
      !model.busy.value &&
      (unknown.value || (!!operation.value && pending(operation.value))),
  );
  const needsRead = computed(
    () => unknown.value || (!!operation.value && pending(operation.value)),
  );
  function accept(value: CloudDelete) {
    operation.value = value;
    unknown.value = false;
    if (value.phase === 'confirmed') completed();
  }
  async function recover() {
    await model.refresh(false, true);
    if (!active()) return;
    const saved = model.view.value?.deletions.find(
      (v) => related(v) && (!known.has(v.id) || pending(v)),
    );
    if (saved) accept(saved);
    // A local read without a record cannot prove that a lost IPC call never ran.
    // Stay read-only; the native journal independently fences overlapping writes.
  }
  async function send() {
    if (!active() || !canSend.value) return;
    working.value = true;
    activity.value = 'send';
    unknown.value = true;
    try {
      const result = await model.command({
        kind: 'delete',
        ids: [...target.ids],
        revision: target.revision,
      });
      if (!active()) return;
      if (result?.deletion) accept(result.deletion);
      else await recover();
    } finally {
      if (active()) working.value = false;
    }
  }
  async function read() {
    if (!active() || !canRead.value) return;
    working.value = true;
    activity.value = 'read';
    try {
      if (operation.value) {
        const result = await model.command({ kind: 'reconcile_delete', id: operation.value.id });
        if (active() && result?.deletion) accept(result.deletion);
      } else await recover();
    } finally {
      if (active()) working.value = false;
    }
  }
  return {
    target,
    operation,
    unknown,
    working,
    activity,
    stale,
    canSend,
    canRead,
    needsRead,
    send,
    read,
  };
}
