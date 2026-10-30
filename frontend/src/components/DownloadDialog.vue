<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import type { DownloadModel } from '../composables/useDownloads';
import type { DownloadDialogTarget, DownloadCommand } from '../services/downloads';
import { defaultDownloadOptions, downloadStatus } from '../services/downloads';
import DownloadOptions from './DownloadOptions.vue';
import Icon from './Icon.vue';
const props = defineProps<{ model: DownloadModel; target: DownloadDialogTarget }>();
const emit = defineEmits<{ close: []; started: [] }>();
const { view, busy, error } = props.model;
const dialog = ref<HTMLDialogElement>(),
  root = ref(view.value?.directory?.root ?? '');
const options = ref({ ...(view.value?.options ?? defaultDownloadOptions) });
if (props.target.kind === 'voice')
  Object.assign(options.value, { container: 'audio', lyrics: false });
const pathType = ref(view.value?.directory?.path_type ?? 0),
  fileType = ref(view.value?.directory?.file_type ?? 1);
const directoryMode = ref<'future' | 'migrate'>('future');
const rename = ref(''),
  destination = ref(''),
  localError = ref(''),
  choosing = ref(false);
const confirmation = ref<'delete' | 'forget' | 'move' | 'rename' | undefined>(
  props.target.kind === 'file' ? props.target.action : undefined,
);
const original =
  props.target.kind === 'file' || props.target.kind === 'cancel' ? props.target.task : undefined;
const owner = view.value?.epoch;
const live = computed(() =>
  view.value?.epoch === owner ? view.value?.tasks.find((t) => t.id === original?.id) : undefined,
);
const task = computed(() => (original ? (live.value ?? original) : undefined));
const stale = computed(
  () =>
    !!original &&
    (!live.value ||
      (props.target.kind === 'cancel'
        ? ['complete', 'cancelled'].includes(live.value.status)
        : live.value.status !== 'complete' || live.value.path !== original.path)),
);
const blocked = computed(
  () =>
    busy.value ||
    choosing.value ||
    !!view.value?.migration ||
    stale.value ||
    (!!original && !!props.model.readError.value) ||
    !!task.value?.fileOperation,
);
const title = computed(() =>
  props.target.kind === 'voice'
    ? `下载 ${props.target.tracks.length} 个单集`
    : props.target.kind === 'cloud'
      ? `下载 ${props.target.tracks.length} 首云盘原文件`
      : props.target.kind === 'start'
        ? `下载 ${props.target.tracks.length} 首歌曲`
        : props.target.kind === 'settings'
          ? '音质与下载'
          : props.target.kind === 'cancel'
            ? '删除下载任务'
            : props.target.kind === 'clear'
              ? '清空正在下载'
              : confirmation.value
                ? {
                    rename: '重命名下载文件',
                    move: '移动下载文件',
                    delete: '删除下载文件',
                    forget: '移出下载列表',
                  }[confirmation.value]
                : '管理下载',
);
let focus: HTMLElement | null = null;
onMounted(() => {
  focus = document.activeElement as HTMLElement;
  dialog.value?.showModal();
  rename.value =
    task.value?.path
      ?.split(/[\\/]/)
      .pop()
      ?.replace(/\.[^.]+$/, '') ??
    task.value?.track.name ??
    '';
  error.value = '';
  if (confirmation.value) void focusForm();
});
onUnmounted(() => focus?.focus());
async function focusForm() {
  await nextTick();
  dialog.value
    ?.querySelector<HTMLElement>('form input:not(:disabled),form footer button[type="button"]')
    ?.focus({ preventScroll: true });
}
watch(confirmation, () => void focusForm());
function close() {
  if (!busy.value && !choosing.value) emit('close');
}
async function choose(kind: 'root' | 'file') {
  choosing.value = true;
  try {
    const value = await props.model.pick();
    if (value) {
      if (kind === 'root') root.value = value;
      else destination.value = value;
    }
  } finally {
    choosing.value = false;
  }
}
async function perform(command: DownloadCommand) {
  if (blocked.value) return;
  const result = await props.model.command(command);
  if (result?.failed.length)
    localError.value = `已受理 ${result.accepted.length} 项，${result.failed.length} 项未完成：${result.failed[0]?.message}`;
  if (result && !result.failed.length) {
    emit('close');
    if (
      command.kind === 'start' ||
      command.kind === 'start_cloud' ||
      command.kind === 'start_voice'
    )
      emit('started');
  }
}
async function save() {
  localError.value = '';
  if (!root.value.trim()) {
    localError.value = '请选择下载目录';
    return;
  }
  if (blocked.value) return;
  const previousRoot = view.value?.directory?.root;
  if (
    props.target.kind === 'settings' &&
    directoryMode.value === 'migrate' &&
    previousRoot &&
    previousRoot !== root.value.trim()
  ) {
    const settings = await props.model.command({
      kind: 'preferences',
      options: { ...options.value },
    });
    if (!settings || settings.failed.length) return;
    const configured = await props.model.command({
      kind: 'configure',
      root: previousRoot,
      path_type: pathType.value,
      file_type: fileType.value,
    });
    if (!configured || configured.failed.length) return;
    await perform({ kind: 'migrate', root: root.value.trim() });
    return;
  }
  const configured = await props.model.command({
    kind: 'configure',
    root: root.value.trim(),
    path_type: pathType.value,
    file_type: fileType.value,
  });
  if (!configured || configured.failed.length) return;
  if (props.target.kind === 'cloud') {
    await perform({
      kind: 'start_cloud',
      song_ids: props.target.tracks.map((t) => t.id),
      revision: props.target.revision,
      lyrics: options.value.lyrics,
    });
    return;
  }
  if (props.target.kind === 'voice') {
    await perform({
      kind: 'start_voice',
      voice_ids: props.target.tracks.map((t) => t.id),
      options: { ...options.value, container: 'audio', lyrics: false },
    });
    return;
  }
  const saved = await props.model.command({ kind: 'preferences', options: { ...options.value } });
  if (!saved || saved.failed.length) return;
  if (props.target.kind === 'start')
    await perform({
      kind: 'start',
      song_ids: props.target.tracks.map((t) => t.id),
      options: { ...options.value },
    });
  else emit('close');
}
async function fileAction() {
  if (!task.value || !confirmation.value || blocked.value || task.value.cleanupPending) return;
  const id = task.value.id;
  switch (confirmation.value) {
    case 'rename':
      if (rename.value.trim())
        await perform({ kind: 'rename', task_id: id, name: rename.value.trim() });
      break;
    case 'move':
      if (destination.value.trim())
        await perform({ kind: 'move_to_directory', task_id: id, root: destination.value.trim() });
      break;
    default:
      await perform({ kind: 'file', task_id: id, action: confirmation.value, target: null });
  }
}
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog download-dialog"
    :class="{
      'download-settings-dialog': ['start', 'settings', 'cloud', 'voice'].includes(target.kind),
    }"
    aria-labelledby="download-dialog-title"
    @cancel.prevent="close"
  >
    <header>
      <h2 id="download-dialog-title">{{ title }}</h2>
      <button
        class="icon-button"
        aria-label="关闭下载对话框"
        :disabled="busy || choosing"
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <div v-if="error || localError" class="error-state" role="alert">{{ localError || error }}</div>
    <p v-if="view?.migration" class="form-warning">目录迁移尚未完成，请在下载管理中继续或撤销。</p>
    <p v-if="stale" class="form-warning" role="alert">
      这条下载记录已改变或不在当前列表中，请关闭窗口后重新选择。
    </p>
    <p v-if="original && model.readError.value" class="form-warning" role="alert">
      暂时无法确认下载状态，请关闭窗口并刷新后重试。
    </p>
    <form
      v-if="
        target.kind === 'settings' ||
        target.kind === 'start' ||
        target.kind === 'cloud' ||
        target.kind === 'voice'
      "
      @submit.prevent="save"
    >
      <div class="download-settings-body">
        <fieldset :disabled="blocked">
          <p
            v-if="target.kind === 'start' || target.kind === 'cloud' || target.kind === 'voice'"
            class="download-selection ellipsis"
            :title="target.tracks.map((t) => t.name).join('、')"
          >
            {{
              target.tracks
                .slice(0, 3)
                .map((t) => t.name)
                .join('、')
            }}{{ target.tracks.length > 3 ? '…' : '' }}
          </p>
          <template v-if="target.kind === 'cloud'">
            <p class="muted">保存云盘原文件，保留原音质和已有标签。</p>
            <label class="private-checkbox"
              ><input type="checkbox" v-model="options.lyrics" /> 另存歌词文件</label
            >
          </template>
          <DownloadOptions v-else v-model="options" :voice="target.kind === 'voice'" />
          <label
            >下载目录
            <div class="download-path">
              <input
                v-model="root"
                aria-label="下载目录"
                placeholder="选择保存音乐的文件夹"
                required
              /><button type="button" class="text-button" @click="choose('root')">选择目录</button>
            </div></label
          >
          <div class="download-naming">
            <label
              >文件夹组织<select v-model.number="pathType" aria-label="文件夹组织">
                <option :value="0">全部放在下载目录</option>
                <option :value="1">按歌手</option>
                <option :value="2">按歌手 / 专辑</option>
              </select></label
            ><label
              >文件命名<select v-model.number="fileType" aria-label="文件命名">
                <option :value="1">歌曲名</option>
                <option :value="0">歌手 - 歌曲名</option>
                <option :value="2">歌曲名 - 歌手</option>
              </select></label
            >
          </div>
          <div
            v-if="
              target.kind === 'settings' && view?.directory && root.trim() !== view.directory.root
            "
            class="download-directory-choice"
          >
            <label class="private-checkbox"
              ><input
                v-model="directoryMode"
                type="radio"
                value="future"
                name="directory-mode"
              />仅新建下载使用此目录</label
            >
            <label class="private-checkbox"
              ><input
                v-model="directoryMode"
                type="radio"
                value="migrate"
                name="directory-mode"
              />同时迁移原目录中的下载任务与文件</label
            >
            <p class="muted">
              迁移会暂时暂停下载，完成后恢复原任务状态。发生冲突时保留进度，可继续或撤销；已有同名文件不会被覆盖。
            </p>
          </div>
          <p class="muted">
            {{
              target.kind === 'cloud' ? '命名设置对新任务生效。' : '音质和命名设置对新任务生效。'
            }}已存在的同名文件会保留，需要在任务中处理冲突。
          </p>
        </fieldset>
      </div>
      <footer>
        <button type="button" class="text-button" :disabled="busy || choosing" @click="close">
          取消</button
        ><button class="primary" :disabled="blocked || !root.trim()">
          {{
            busy
              ? '正在处理…'
              : target.kind === 'start' || target.kind === 'cloud' || target.kind === 'voice'
                ? '开始下载'
                : directoryMode === 'migrate' && view?.directory?.root !== root.trim()
                  ? '保存并迁移'
                  : '保存设置'
          }}
        </button>
      </footer>
    </form>
    <template v-else-if="target.kind === 'cancel'">
      <p>确定将该资源从列表中删除？</p>
      <p>{{ task?.track.name }}</p>
      <p class="muted">取消这个下载任务，并清理它创建的临时文件。</p>
      <footer>
        <button class="text-button" :disabled="busy" @click="close">返回</button
        ><button
          class="primary"
          :disabled="blocked"
          @click="perform({ kind: 'control', task_ids: [target.task.id], action: 'cancel' })"
        >
          确认删除任务
        </button>
      </footer>
    </template>
    <template v-else-if="target.kind === 'clear'"
      ><p>取消本账号全部未完成的下载，并清理这些任务创建的临时文件。已完成的歌曲会保留。</p>
      <footer>
        <button class="text-button" :disabled="busy" @click="close">返回</button
        ><button
          class="primary"
          :disabled="blocked"
          @click="perform({ kind: 'all', action: 'cancel' })"
        >
          清空正在下载
        </button>
      </footer></template
    >
    <template v-else-if="task">
      <h3>{{ task.track.name }}</h3>
      <p class="muted">{{ downloadStatus(task) }}</p>
      <p class="download-full-path">{{ task.path }}</p>
      <div v-if="!confirmation" class="download-file-actions">
        <button class="download-button" :disabled="blocked" @click="model.open(task.id)">
          打开所在目录</button
        ><button
          class="download-button"
          :disabled="blocked || task.cleanupPending || !!task.fileOperation"
          @click="confirmation = 'rename'"
        >
          重命名</button
        ><button
          class="download-button"
          :disabled="blocked || task.cleanupPending || !!task.fileOperation"
          @click="confirmation = 'move'"
        >
          移动到…</button
        ><button
          class="download-button"
          :disabled="blocked || task.cleanupPending || !!task.fileOperation"
          @click="confirmation = 'forget'"
        >
          仅移出下载列表</button
        ><button
          class="download-button danger"
          :disabled="blocked || task.cleanupPending || !!task.fileOperation"
          @click="confirmation = 'delete'"
        >
          删除本地文件</button
        ><button
          v-if="task.cleanupPending"
          class="download-button"
          :disabled="blocked"
          @click="perform({ kind: 'control', task_ids: [task.id], action: 'repair' })"
        >
          重试歌词与文件整理
        </button>
      </div>
      <form v-else @submit.prevent="fileAction">
        <label v-if="confirmation === 'rename'"
          >新文件名（保留原扩展名）<input
            v-model="rename"
            aria-label="新文件名"
            required
            maxlength="240"
            :disabled="blocked"
        /></label>
        <label v-if="confirmation === 'move'"
          >目标文件夹
          <div class="download-path">
            <input
              v-model="destination"
              aria-label="移动目标目录"
              required
              :disabled="blocked"
            /><button type="button" class="text-button" :disabled="blocked" @click="choose('file')">
              选择目录
            </button>
          </div></label
        >
        <p v-if="confirmation === 'forget'">从下载列表移除此记录。本地文件和已有播放记录会保留。</p>
        <p v-if="confirmation === 'delete'" class="form-warning">
          {{
            task?.source === 'mv'
              ? '将删除该 MV 的本地视频文件，同时清理对应的下载与本地库记录。此操作无法撤销。'
              : '将删除这首歌的本地音频及关联歌词，同时清理本地库和播放队列中的对应记录。此操作无法撤销。'
          }}
        </p>
        <p v-if="confirmation === 'move' || confirmation === 'rename'" class="muted">
          同步更新歌词、本地库与播放队列。若目标已有文件，操作会停止并保留原文件。
        </p>
        <footer>
          <button
            type="button"
            class="text-button"
            :disabled="busy || choosing"
            @click="confirmation = undefined"
          >
            返回</button
          ><button
            class="primary"
            :disabled="
              blocked ||
              task.cleanupPending ||
              (confirmation === 'rename' && !rename.trim()) ||
              (confirmation === 'move' && !destination.trim())
            "
          >
            {{ busy ? '正在处理…' : confirmation === 'delete' ? '确认删除本地文件' : '确认' }}
          </button>
        </footer>
      </form>
    </template>
  </dialog>
</template>
