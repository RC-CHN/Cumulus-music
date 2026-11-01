import { onScopeDispose, ref, watch } from 'vue';
import { failure } from '../services/contracts.ts';
import type { CloudClient, CloudQuery } from '../services/cloud';
import { readCloudCollection, type CloudCollectionData } from '../services/cloudCollection.ts';
export interface CloudToolbarSource {
  epoch: string;
  revision: string;
  query: CloudQuery;
}
export type CloudToolbarAction = 'queue' | 'download';
const identity = (value: CloudToolbarSource | undefined) =>
  value && JSON.stringify([value.epoch, value.revision, value.query]);
export function useCloudToolbar(
  client: Pick<CloudClient, 'cloudPage'>,
  source: () => CloudToolbarSource | undefined,
  perform: (
    action: CloudToolbarAction,
    data: CloudCollectionData,
    query: CloudQuery,
  ) => Promise<boolean>,
  expired: () => void,
) {
  const busy = ref(false),
    message = ref(''),
    error = ref(''),
    needsRefresh = ref(false);
  let alive = true,
    serial = 0;
  function clear() {
    serial++;
    busy.value = false;
    message.value = '';
    error.value = '';
    needsRefresh.value = false;
  }
  watch(() => identity(source()), clear);
  onScopeDispose(() => {
    alive = false;
    clear();
  });
  async function run(action: CloudToolbarAction) {
    const current = source();
    if (!alive || busy.value || !current) return;
    const captured = { ...current, query: { ...current.query } },
      expected = identity(captured),
      attempt = ++serial,
      active = () => alive && attempt === serial && identity(source()) === expected;
    busy.value = true;
    error.value = '';
    message.value = '正在读取完整歌曲列表…';
    needsRefresh.value = false;
    try {
      const data = await readCloudCollection(
        client,
        captured.epoch,
        captured.query,
        active,
        (loaded, total) => {
          if (active()) message.value = `正在读取歌曲：${loaded} / ${total}`;
        },
        {
          revision: captured.revision,
          maximum: action === 'download' ? 5000 : 10000,
          action: action === 'download' ? '下载' : '添加',
        },
      );
      if (!active()) return;
      if (!data.tracks.length)
        throw { code: 'empty', message: '当前搜索没有歌曲，请调整搜索条件。' };
      message.value = action === 'queue' ? '正在添加至播放列表…' : '正在准备下载…';
      const accepted = await perform(action, data, captured.query);
      if (active())
        message.value = accepted
          ? action === 'queue'
            ? '已添加至播放列表'
            : ''
          : '本次操作已更新或取消';
    } catch (e) {
      if (active()) {
        const issue = failure(e);
        error.value = issue.message;
        message.value = '';
        needsRefresh.value = ['snapshot_changed', 'queue_changed', 'conflict'].includes(issue.code);
        if (['session_expired', 'profile_changed'].includes(issue.code)) expired();
      }
    } finally {
      if (active()) busy.value = false;
    }
  }
  return { busy, message, error, needsRefresh, run, clear };
}
