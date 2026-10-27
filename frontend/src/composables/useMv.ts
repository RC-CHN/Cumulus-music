import { ref, shallowRef } from 'vue';
import type { AccountView, CoreClient } from '../services/contracts.ts';
import { failure } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import { useMvSocial } from './useMvSocial.ts';
import type { MvDetail, MvPage, MvQuery, VideoAction, VideoView } from '../services/mv.ts';

export function useMv(core: CoreClient = useCore(), expired: () => void = () => {}) {
  const social = useMvSocial(core, expired);
  const epoch = ref(''),
    page = shallowRef<MvPage>(),
    detail = shallowRef<MvDetail>(),
    video = shallowRef<VideoView>();
  const query = ref<MvQuery>({ keyword: '', offset: 0, limit: 24, revision: null });
  const loading = ref(false),
    detailLoading = ref(false),
    busy = ref(false),
    error = ref(''),
    videoError = ref(''),
    videoReadError = ref(''),
    notice = ref('');
  let generation = 0,
    listing = 0,
    selecting = 0,
    commanding = 0,
    visible = false,
    target = '',
    timer: ReturnType<typeof setTimeout> | undefined;
  function valid(g: number, e: string) {
    return generation === g && epoch.value === e && !!e;
  }
  function problem(value: unknown, channel = error) {
    const issue = failure(value);
    channel.value = issue.message;
    if (issue.code === 'session_expired' || issue.code === 'profile_changed') {
      dispose();
      expired();
    }
  }
  function accept(value: VideoView) {
    if (value.epoch !== epoch.value) return;
    if (video.value && BigInt(value.revision) < BigInt(video.value.revision)) return;
    video.value = value;
  }
  async function poll(g: number, e: string) {
    try {
      const value = await core.videoSnapshot(e);
      if (valid(g, e)) {
        accept(value);
        videoReadError.value = '';
      }
    } catch (value) {
      if (valid(g, e)) problem(value, videoReadError);
    }
    if (valid(g, e)) timer = setTimeout(() => void poll(g, e), video.value?.id ? 200 : 700);
  }
  function dispose() {
    social.dispose();
    generation++;
    listing++;
    selecting++;
    commanding++;
    clearTimeout(timer);
    epoch.value = '';
    page.value = undefined;
    detail.value = undefined;
    video.value = undefined;
    loading.value = false;
    detailLoading.value = false;
    busy.value = false;
    target = '';
  }
  function connect(account: AccountView) {
    dispose();
    social.connect(account);
    error.value = '';
    videoError.value = '';
    videoReadError.value = '';
    notice.value = '';
    if (account.status !== 'authenticated') return;
    epoch.value = account.epoch;
    void poll(generation, epoch.value);
    if (visible) void load(true);
  }
  async function load(refresh = false) {
    if (!epoch.value) return;
    const attempt = ++listing,
      g = generation,
      e = epoch.value;
    loading.value = true;
    error.value = '';
    try {
      const value = await core.mvPage(
        e,
        { ...query.value, revision: refresh ? null : query.value.revision },
        refresh,
      );
      if (valid(g, e) && attempt === listing && value.epoch === e) {
        page.value = value;
        query.value = { ...query.value, revision: value.revision };
        if (value.offset > 0 && value.offset >= value.total) {
          query.value = {
            ...query.value,
            offset:
              Math.floor(Math.max(0, value.total - 1) / query.value.limit) * query.value.limit,
          };
          await load();
        }
      }
    } catch (value) {
      if (valid(g, e) && attempt === listing) {
        problem(value);
        // A failed remote read must still expose the durable pending journal.
        // Keep its error visible while recovering the last complete snapshot.
        if (refresh && valid(g, e)) {
          try {
            const cached = await core.mvPage(e, { ...query.value, revision: null }, false);
            if (valid(g, e) && attempt === listing && cached.epoch === e) {
              page.value = cached;
              query.value = { ...query.value, revision: cached.revision };
            }
          } catch {
            /* The original refresh failure remains visible. */
          }
        }
      }
    } finally {
      if (valid(g, e) && attempt === listing) loading.value = false;
    }
  }
  function show(value: boolean) {
    visible = value;
    if (value && epoch.value) void load(true);
  }
  function filter(keyword: string) {
    query.value = { ...query.value, keyword, offset: 0, revision: null };
    void load();
  }
  function paginate(offset: number) {
    query.value = { ...query.value, offset };
    void load();
  }
  async function action(command: VideoAction) {
    const g = generation,
      e = epoch.value,
      request = ++commanding;
    if (!e) return;
    videoError.value = '';
    try {
      const value = await core.videoAction(e, command);
      if (valid(g, e)) accept(value);
    } catch (value) {
      if (valid(g, e) && request === commanding) problem(value, videoError);
    }
  }
  async function open(id: string, taskId?: string) {
    const g = generation,
      e = epoch.value,
      attempt = ++selecting;
    if (!e) return;
    target = id;
    const socialTicket = social.activate(id);
    detail.value = undefined;
    detailLoading.value = true;
    error.value = '';
    notice.value = '';
    // Start the native load immediately. A slow metadata response cannot later
    // start an older selection after the user has chosen another MV.
    if (video.value?.id !== id || video.value.state === 'idle' || taskId)
      void action({ kind: 'open', id, resolution: 480, download_task_id: taskId });
    try {
      const value = await core.mvDetail(e, id);
      if (valid(g, e) && selecting === attempt && value.epoch === e && target === id) {
        detail.value = value.value;
        social.acceptDetail(value.value, socialTicket);
      }
    } catch (value) {
      if (valid(g, e) && selecting === attempt) problem(value);
    } finally {
      if (valid(g, e) && selecting === attempt) detailLoading.value = false;
    }
  }
  async function collect(id: string, desired: boolean, reconcile = false) {
    if (busy.value || !epoch.value) return;
    const g = generation,
      e = epoch.value;
    busy.value = true;
    error.value = '';
    notice.value = '';
    let operationError = '';
    try {
      const result = await (reconcile ? core.mvReconcile(e, id) : core.mvCollect(e, id, desired));
      if (!valid(g, e) || result.epoch !== e) return;
      const observed =
        result.value.observed ?? (result.value.phase === 'unchanged' ? result.value.desired : null);
      if (detail.value?.card.id === id && observed !== null)
        detail.value = { ...detail.value, card: { ...detail.value.card, subscribed: observed } };
      notice.value =
        result.value.phase === 'confirmed' || result.value.phase === 'unchanged'
          ? '收藏状态已确认'
          : '操作结果待核对';
    } catch (value) {
      if (valid(g, e)) {
        problem(value);
        operationError = error.value;
      }
    } finally {
      if (valid(g, e)) {
        busy.value = false;
        await load(true);
        if (valid(g, e) && operationError) error.value = operationError;
      }
    }
  }
  async function download(resolution: number) {
    if (busy.value || !detail.value || !epoch.value) return;
    const g = generation,
      e = epoch.value,
      id = detail.value.card.id;
    busy.value = true;
    error.value = '';
    notice.value = '';
    try {
      const value = await core.downloadCommand(e, { kind: 'start_mv', id, resolution });
      if (valid(g, e) && value.epoch === e)
        notice.value = value.accepted.length
          ? '已加入下载队列，可在下载管理中查看'
          : '未能创建下载任务';
    } catch (value) {
      if (valid(g, e)) problem(value);
    } finally {
      if (valid(g, e)) busy.value = false;
    }
  }
  return {
    social,
    epoch,
    page,
    detail,
    video,
    query,
    loading,
    detailLoading,
    busy,
    error,
    videoError,
    videoReadError,
    notice,
    connect,
    dispose,
    load,
    show,
    filter,
    paginate,
    action,
    open,
    collect,
    download,
    frame: (...args: Parameters<CoreClient['videoFrame']>) => core.videoFrame(...args),
  };
}
export type MvModel = ReturnType<typeof useMv>;
