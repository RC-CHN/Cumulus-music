import { computed, onScopeDispose, ref, shallowRef, watch } from 'vue';
import type { CloudModel } from './useCloud.ts';
import type { CloudClient, CloudUploadRemoval } from '../services/cloud.ts';
import type { CloudTaskTarget } from '../services/cloudTasks.ts';
import { failure } from '../services/contracts.ts';
export function useCloudTaskRemoval(
  model: CloudModel,
  client: Pick<CloudClient, 'cloudUploadRemovals'>,
  target: CloudTaskTarget,
  completed: () => void,
) {
  const working = ref(false),
    sent = ref(false),
    error = ref(''),
    items = shallowRef<CloudUploadRemoval['items']>([]),
    failures = shallowRef<Record<string, string>>({});
  let alive = true;
  const active = () => alive && model.accountEpoch.value === target.epoch;
  onScopeDispose(() => {
    alive = false;
    items.value = [];
    failures.value = {};
  });
  watch(
    () => model.accountEpoch.value,
    () => {
      if (!active()) {
        items.value = [];
        failures.value = {};
        error.value = '';
      }
    },
    { flush: 'sync' },
  );
  const removed = computed(() => items.value.filter((item) => item.removed).length),
    blocked = computed(() => !active() || working.value || model.busy.value);
  async function inspect() {
    const next: CloudUploadRemoval['items'] = [];
    for (let offset = 0; offset < target.ids.length; offset += 5000) {
      if (!active()) return;
      const ids = target.ids.slice(offset, offset + 5000),
        view = await client.cloudUploadRemovals(target.epoch, ids);
      if (!active()) return;
      if (
        view.epoch !== target.epoch ||
        view.items.length !== ids.length ||
        new Set(view.items.map((item) => item.id)).size !== ids.length ||
        view.items.some((item) => !ids.includes(item.id))
      )
        throw { message: '移除状态不完整，请重新核对', code: 'unavailable' };
      next.push(...view.items);
    }
    if (!active()) return;
    items.value = next;
    if (next.length === target.ids.length && next.every((item) => item.removed)) {
      model.notice.value = `已移除 ${target.ids.length} 个上传任务`;
      model.error.value = '';
      completed();
    }
  }
  async function read() {
    if (blocked.value) return;
    working.value = true;
    error.value = '';
    try {
      await inspect();
    } catch (e) {
      if (active()) error.value = failure(e).message;
    } finally {
      if (active()) working.value = false;
    }
  }
  async function send() {
    if (blocked.value) return;
    working.value = true;
    error.value = '';
    const retry = sent.value || !!target.retry;
    sent.value = true;
    const remaining = target.ids.filter(
      (id) => !items.value.some((item) => item.id === id && item.removed),
    );
    try {
      const rejected: Record<string, string> = {};
      for (let offset = 0; offset < remaining.length; offset += 5000) {
        if (!active()) return;
        const ids = remaining.slice(offset, offset + 5000),
          result = await model.command({ kind: 'remove_uploads', ids, retry });
        if (!active()) return;
        if (!result || result.epoch !== target.epoch) {
          error.value = '移除回执尚未确认，请先只读核对；重试会保留本次任务范围。';
          break;
        }
        for (const failed of result.failed)
          if (ids.includes(failed.id)) rejected[failed.id] = failed.message;
      }
      if (active()) {
        failures.value = rejected;
        await inspect();
      }
    } catch (e) {
      if (active()) error.value = failure(e).message;
    } finally {
      if (active()) working.value = false;
    }
  }
  return { active, working, sent, error, items, failures, removed, blocked, send, read };
}
