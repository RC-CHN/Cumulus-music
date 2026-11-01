import { onScopeDispose, ref, shallowRef, watch } from 'vue';
import type { CloudClient, CloudResource } from '../services/cloud.ts';
import type { CloudMenuTarget } from '../services/cloudMenu.ts';
import { failure } from '../services/contracts.ts';
export function useCloudResource(
  client: Pick<CloudClient, 'cloudResource'>,
  epoch: () => string,
  revision: () => string | null | undefined,
  expired: () => void,
) {
  const target = shallowRef<CloudMenuTarget>(),
    resource = shallowRef<CloudResource>(),
    loading = ref(false),
    error = ref(''),
    needsRefresh = ref(false);
  let alive = true,
    serial = 0;
  const active = () =>
    alive &&
    !!target.value &&
    target.value.epoch === epoch() &&
    target.value.revision === revision();
  function clear() {
    serial++;
    target.value = undefined;
    resource.value = undefined;
    loading.value = false;
    error.value = '';
    needsRefresh.value = false;
  }
  watch([epoch, revision], () => {
    if (!active()) clear();
  });
  onScopeDispose(() => {
    alive = false;
    clear();
  });
  async function read() {
    const captured = target.value;
    if (!captured || !active() || loading.value) return;
    const attempt = ++serial;
    loading.value = true;
    error.value = '';
    resource.value = undefined;
    needsRefresh.value = false;
    try {
      const value = await client.cloudResource(
        captured.epoch,
        captured.clickedId,
        captured.revision,
      );
      if (!active() || attempt !== serial) return;
      if (
        value.epoch !== captured.epoch ||
        value.revision !== captured.revision ||
        value.track.track.id !== captured.clickedId
      )
        throw { code: 'queue_changed', message: '云盘列表已变化，请刷新后重新打开菜单' };
      resource.value = value;
    } catch (e) {
      if (active() && attempt === serial) {
        const issue = failure(e);
        error.value = issue.message;
        needsRefresh.value = ['stale', 'queue_changed', 'conflict'].includes(issue.code);
        if (['session_expired', 'profile_changed'].includes(issue.code)) expired();
      }
    } finally {
      if (active() && attempt === serial) loading.value = false;
    }
  }
  function set(value: CloudMenuTarget) {
    clear();
    target.value = value;
  }
  return { target, resource, loading, error, needsRefresh, active, clear, set, read };
}
