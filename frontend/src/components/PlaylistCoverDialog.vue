<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import type { AccountView } from '../services/contracts';
import type { PlaylistEditor } from '../services/playlists';
import {
  coverEditable,
  coverMessage,
  coverTerminal,
  coverBusyMessage,
} from '../services/playlistCover';
import {
  coverPosition,
  coverCrop,
  coverZoomLimits,
  dragCover,
  restoreCoverPosition,
  type CropPosition,
} from '../services/coverGeometry';
import type { PlaylistCoverModel } from '../composables/usePlaylistCover';
import Icon from './Icon.vue';
const props = defineProps<{
  account: AccountView;
  editor: PlaylistEditor;
  model: PlaylistCoverModel;
}>();
const emit = defineEmits<{ close: [] }>();
const model = props.model;
const {
  task,
  image,
  error,
  warning,
  busy,
  loading,
  previewing,
  readFailed,
  stopping,
  changedElsewhere,
  latest,
} = model;
const dialog = ref<HTMLDialogElement>();
const body = ref<HTMLElement>();
const position = ref<CropPosition>({ x: 0, y: 0, zoom: 1 });
const editable = computed(
  () => !!task.value && coverEditable(task.value) && !!image.value && !busy.value && !loading.value,
);
const limits = computed(() =>
  image.value ? coverZoomLimits(image.value.width, image.value.height) : { min: 1, max: 10 },
);
const zoomProgress = computed(
  () =>
    `${((position.value.zoom - limits.value.min) / (limits.value.max - limits.value.min || 1)) * 100}%`,
);
const crop = computed(() =>
  image.value ? coverCrop(image.value.width, image.value.height, position.value) : undefined,
);
const imageStyle = computed(() => {
  const i = image.value;
  if (!i) return {};
  const scale = (362 / i.width) * position.value.zoom;
  return {
    width: `${i.width * scale}px`,
    height: `${i.height * scale}px`,
    transform: `translate(${-position.value.x * scale}px,${-position.value.y * scale}px)`,
  };
});
const ambiguous = computed(
  () => task.value?.phase === 'allocating' || task.value?.transfer === 'ambiguous',
);
const finished = computed(() => !!task.value && coverTerminal(task.value));
let pointer: { id: number; x: number; y: number } | undefined;
watch(
  image,
  async (value) => {
    if (!value) return;
    position.value = task.value?.crop
      ? restoreCoverPosition(value.width, value.height, task.value.crop)
      : coverPosition(value.width, value.height);
    if (task.value && coverEditable(task.value)) {
      await nextTick();
      // A newly selected image starts at its crop area; cancelling the native
      // picker does not change image, so it keeps the previous viewport intact.
      if (body.value) body.value.scrollTop = 0;
    }
  },
  { immediate: true },
);
watch(
  [busy, error, warning, readFailed, () => task.value?.phase, () => task.value?.transfer, latest],
  async () => {
    if (
      busy.value ||
      error.value ||
      warning.value ||
      readFailed.value ||
      (task.value && !coverEditable(task.value))
    ) {
      await nextTick();
      // Keep progress/errors and recovery actions visible in short windows.
      // Scroll only this dialog body, preserving the editor and crop coordinates.
      if (body.value) body.value.scrollTop = body.value.scrollHeight;
    }
  },
  { immediate: true },
);
// Match the official successful-save exit, while keeping warnings/recovery
// visible until they are resolved or explicitly dismissed.
watch([busy, loading, task, warning, error, readFailed], () => {
  if (
    task.value?.phase === 'confirmed' &&
    !task.value.cleanupPending &&
    !busy.value &&
    !loading.value &&
    !warning.value &&
    !error.value &&
    !readFailed.value
  )
    emit('close');
});
onMounted(() => {
  dialog.value?.showModal();
});
function zoom(value: number) {
  if (editable.value && image.value)
    position.value = coverPosition(image.value.width, image.value.height, {
      ...position.value,
      zoom: value,
    });
}
function move(dx: number, dy: number) {
  if (editable.value && image.value)
    position.value = dragCover(image.value.width, image.value.height, position.value, dx, dy);
}
function down(event: PointerEvent) {
  if (!editable.value || event.button !== 0) return;
  pointer = { id: event.pointerId, x: event.clientX, y: event.clientY };
  (event.currentTarget as HTMLElement).focus();
  try {
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  } catch {
    /* A pointer can end before capture. */
  }
}
function drag(event: PointerEvent) {
  if (pointer?.id === event.pointerId) {
    move(event.clientX - pointer.x, event.clientY - pointer.y);
    pointer = { id: event.pointerId, x: event.clientX, y: event.clientY };
  }
}
function key(event: KeyboardEvent) {
  const step = event.shiftKey ? 20 : 2;
  const direction: Record<string, [number, number]> = {
    ArrowLeft: [-step, 0],
    ArrowRight: [step, 0],
    ArrowUp: [0, -step],
    ArrowDown: [0, step],
  };
  if (direction[event.key]) {
    event.preventDefault();
    move(...direction[event.key]!);
  } else if (event.key === '+' || event.key === '=') {
    event.preventDefault();
    zoom(position.value.zoom + 0.1);
  } else if (event.key === '-') {
    event.preventDefault();
    zoom(position.value.zoom - 0.1);
  }
}
async function close() {
  if (busy.value || loading.value) return;
  if (task.value && coverEditable(task.value)) {
    const result = await model.command({ kind: 'abandon' });
    if (result?.task.phase !== 'abandoned') return;
  }
  emit('close');
}
async function retryPreview(event: MouseEvent) {
  const trigger = event.currentTarget;
  const focused = document.activeElement === trigger;
  if (!(await model.preview())) return;
  await nextTick();
  // Retry disappears on success. Restore keyboard access without taking focus
  // away from another action the user chose during the read.
  if (focused && (document.activeElement === trigger || document.activeElement === document.body))
    dialog.value?.querySelector<HTMLElement>('.cover-crop-area')?.focus({ preventScroll: true });
}
async function save() {
  if (!task.value) return;
  if (coverEditable(task.value) && crop.value)
    await model.command({ kind: 'save', crop: crop.value });
  else if (task.value.phase === 'binding') await model.command({ kind: 'reconcile' });
  else if (task.value.phase === 'allocation_rejected')
    await model.command({ kind: 'retry_allocation' });
  else await model.command({ kind: 'continue' });
}
async function applyLatest() {
  if (!latest.value || !task.value) return;
  const result = await model.command({ kind: 'rebase', revision: latest.value.revision });
  if (result) await model.command({ kind: 'continue' });
}
</script>
<template>
  <dialog
    ref="dialog"
    class="cover-dialog"
    aria-labelledby="cover-dialog-title"
    :data-phase="task?.phase ?? 'empty'"
    :data-crop="JSON.stringify(crop)"
    @cancel.prevent="close"
  >
    <header>
      <h2 id="cover-dialog-title">编辑图片</h2>
      <button
        class="icon-button"
        aria-label="关闭封面编辑"
        :disabled="busy || loading"
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <div ref="body" class="cover-body">
      <p v-if="loading" class="muted" role="status">正在读取封面编辑进度…</p>
      <div
        v-if="image"
        class="cover-crop-area"
        :class="{ editable }"
        role="group"
        tabindex="0"
        :aria-disabled="!editable"
        aria-label="封面裁剪区域，方向键移动图片，加减键缩放"
        @pointerdown="down"
        @pointermove="drag"
        @pointerup="pointer = undefined"
        @pointercancel="pointer = undefined"
        @lostpointercapture="pointer = undefined"
        @keydown="key"
        @wheel.prevent="zoom(position.zoom * Math.exp(-$event.deltaY * 0.001))"
      >
        <img
          class="cover-crop-image"
          :src="image.preview"
          :style="imageStyle"
          alt="所选封面"
          draggable="false"
        />
        <div class="cover-crop-mask" aria-hidden="true"></div>
      </div>
      <div v-else class="cover-empty">
        <img v-if="editor.coverUrl" :src="editor.coverUrl" alt="当前歌单封面" />
        <Icon v-else name="music" />
        <p :role="previewing ? 'status' : undefined">
          {{
            finished
              ? '本次封面编辑已结束'
              : previewing
                ? '正在读取图片预览…'
                : task
                  ? '暂时无法显示所选图片'
                  : '选择一张图片作为歌单封面'
          }}
        </p>
      </div>
      <label v-if="image" class="cover-zoom" :style="{ '--cover-zoom-progress': zoomProgress }"
        ><span class="sr-only">封面缩放</span
        ><input
          type="range"
          aria-label="封面缩放"
          :min="limits.min"
          :max="limits.max"
          step="0.01"
          :value="position.zoom"
          :aria-valuetext="`${position.zoom.toFixed(2)} 倍`"
          :disabled="!editable"
          @input="zoom(Number(($event.target as HTMLInputElement).value))"
      /></label>
      <p v-if="busy" class="cover-status" role="status">{{ coverBusyMessage(task, stopping) }}</p>
      <p v-else-if="task && !coverEditable(task)" class="cover-status" role="status">
        {{ coverMessage(task) }}
      </p>
      <p v-if="editable" class="cover-tip">
        *鼠标放置在图片上可拖拽移动图片位置
        <span class="sr-only">方向键移动，加减键缩放</span>
      </p>
      <div v-if="task && task.total > 0 && task.phase === 'uploading'" class="cover-progress">
        <div>
          <span>上传进度</span
          ><span>{{ Math.min(100, Math.floor((task.bytes / task.total) * 100)) }}%</span>
        </div>
        <progress :value="task.bytes" :max="task.total" aria-label="封面上传进度"></progress>
      </div>
      <p v-if="error || readFailed" class="error-state" role="alert">
        {{ error || '无法读取封面编辑进度，请重试。' }}
      </p>
      <p v-if="warning" class="cover-warning" role="status">{{ warning }}</p>
      <div v-if="changedElsewhere" class="cover-latest">
        <button class="text-button" :disabled="busy || loading" @click="model.inspectLatest">
          查看最新歌单信息
        </button>
        <template v-if="latest"
          ><p>当前歌单：{{ latest.name }}</p>
          <p>{{ latest.description || '暂无简介' }}</p>
          <button
            v-if="task"
            class="text-button"
            :disabled="busy || !latest.owned || latest.favorite"
            @click="applyLatest"
          >
            确认将这张封面应用到最新歌单
          </button>
          <p v-else class="muted">请关闭此窗口并重新读取歌单后选图。</p>
        </template>
      </div>
      <div class="cover-secondary">
        <button
          v-if="readFailed"
          class="text-button"
          :disabled="busy || loading"
          @click="model.connect(account, editor.id)"
        >
          重新读取封面进度
        </button>
        <button
          v-if="!task || coverEditable(task) || finished"
          class="text-button"
          :disabled="busy || loading || previewing || readFailed"
          @click="model.choose(editor.revision)"
        >
          {{ image || (task && !finished) ? '重新选取图片' : '选择图片' }}
        </button>
        <button
          v-if="task && !finished && !coverEditable(task)"
          class="text-button"
          :disabled="busy || task.phase === 'binding'"
          @click="model.command({ kind: 'abandon' })"
        >
          放弃本次换图
        </button>
        <button
          v-if="task?.transfer === 'error'"
          class="text-button"
          :disabled="busy"
          @click="model.command({ kind: 'renew' })"
        >
          重新获取上传权限
        </button>
        <button
          v-if="task?.cleanupPending"
          class="text-button"
          :disabled="busy"
          @click="model.command({ kind: 'cleanup' })"
        >
          重试清理图片缓存
        </button>
        <button
          v-if="task && !image && !finished"
          class="text-button"
          :disabled="busy || loading || previewing"
          @click="retryPreview"
        >
          {{ previewing ? '正在读取预览…' : '重新读取预览' }}
        </button>
      </div>
      <p v-if="!image && !finished" class="cover-tip">
        支持 JPEG、PNG、BMP、GIF，不超过 5 MB，宽高至少 320 像素。
      </p>
    </div>
    <footer>
      <button class="cover-cancel" :disabled="busy || loading" @click="close">
        {{ finished ? '完成' : task && !coverEditable(task) ? '稍后处理' : '取消' }}
      </button>
      <button
        v-if="busy && task?.phase === 'uploading' && task.transfer !== 'uploaded'"
        class="primary"
        :disabled="stopping"
        @click="model.pause"
      >
        {{ stopping ? '正在暂停…' : '暂停上传' }}
      </button>
      <button
        v-else-if="!finished"
        class="primary"
        :disabled="
          busy ||
          loading ||
          previewing ||
          !task ||
          ambiguous ||
          changedElsewhere ||
          (coverEditable(task) && !image)
        "
        @click="save"
      >
        {{
          busy
            ? '正在保存…'
            : task?.phase === 'binding'
              ? '核对结果'
              : task?.phase === 'allocation_rejected'
                ? '重试上传'
                : task?.phase === 'uploading'
                  ? '继续保存'
                  : '保存'
        }}
      </button>
    </footer>
  </dialog>
</template>
<style scoped>
.cover-dialog {
  width: 442px;
  max-width: calc(100vw - 24px);
  max-height: calc(100vh - 24px);
  min-height: min(556px, calc(100vh - 24px));
  padding: 40px;
  border: 0;
  border-radius: 16px;
  background: var(--surface-raised);
  color: var(--text);
  box-shadow: 0 18px 70px #0003;
  overflow: hidden;
}
.cover-dialog[open] {
  display: flex;
  flex-direction: column;
}
.cover-dialog::backdrop {
  background: transparent;
}
header {
  position: relative;
  flex-shrink: 0;
  text-align: center;
}
h2 {
  font-size: 20px;
  line-height: 28px;
  margin: 0;
  font-weight: 600;
}
header .icon-button {
  position: absolute;
  right: -20px;
  top: -20px;
  width: 24px;
  height: 24px;
  color: var(--text-secondary);
}
header .icon {
  width: 24px;
  height: 24px;
}
.cover-body {
  flex: 1 1 auto;
  min-height: 0;
  margin-top: 40px;
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-width: thin;
}
.cover-crop-area {
  width: 362px;
  max-width: 100%;
  height: 260px;
  position: relative;
  overflow: hidden;
  border-radius: 10px;
  background: var(--surface-hover);
  touch-action: none;
  outline-offset: -2px;
}
.cover-crop-area.editable {
  cursor: grab;
}
.cover-crop-area.editable:active {
  cursor: grabbing;
}
.cover-crop-area:focus-visible {
  outline: 2px solid var(--accent);
}
.cover-crop-image {
  position: absolute;
  left: 50%;
  top: 50%;
  max-width: none;
  max-height: none;
  pointer-events: none;
  user-select: none;
  transform-origin: 0 0;
}
.cover-crop-mask {
  position: absolute;
  left: 50%;
  top: 50%;
  width: 200px;
  height: 200px;
  transform: translate(-50%, -50%);
  border: 1px solid #ffffffb0;
  border-radius: 18px;
  box-shadow: 0 0 0 500px #0005;
  pointer-events: none;
}
.cover-empty {
  height: 260px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-direction: column;
  gap: 12px;
  color: var(--text-secondary);
  background: var(--surface-alt);
  border-radius: 10px;
}
.cover-empty img {
  width: 160px;
  height: 160px;
  object-fit: cover;
  border-radius: 12px;
}
.cover-empty p {
  font-size: 13px;
  margin: 0;
}
.cover-empty .icon {
  width: 64px;
  height: 64px;
}
.cover-zoom {
  display: flex;
  align-items: center;
  height: 18px;
  margin: 20px 0 0;
}
.cover-zoom input {
  appearance: none;
  -webkit-appearance: none;
  width: 100%;
  height: 18px;
  margin: 0;
  background: transparent;
  border-radius: 9px;
  outline-offset: -2px;
}
.cover-zoom input::-webkit-slider-runnable-track {
  height: 8px;
  border-radius: 4px;
  background: linear-gradient(
    to right,
    var(--accent-fill) var(--cover-zoom-progress),
    var(--surface-hover) var(--cover-zoom-progress)
  );
}
.cover-zoom input::-webkit-slider-thumb {
  appearance: none;
  -webkit-appearance: none;
  width: 18px;
  height: 18px;
  margin-top: -5px;
  border: 1px solid var(--line);
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 2px #0003;
}
.cover-zoom input:disabled {
  opacity: 0.5;
  cursor: default;
}
.cover-tip,
.cover-status,
.muted {
  font-size: 13px;
  line-height: 20px;
  color: var(--text-secondary);
  margin: 20px 0 0;
}
.cover-status {
  padding-right: 8px;
  color: var(--text-secondary);
}
.error-state {
  font-size: 13px;
  line-height: 20px;
  color: var(--danger);
  margin: 10px 0;
}
.cover-secondary {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 12px;
}
.cover-secondary:empty {
  display: none;
}
.text-button {
  font-size: 13px;
  color: var(--accent);
  padding: 2px 0;
  line-height: 20px;
  outline-offset: -2px;
  border-radius: 4px;
}
.cover-warning {
  background: var(--warning-bg);
  color: var(--warning);
  border-radius: 8px;
  padding: 10px 12px;
  font-size: 13px;
  line-height: 20px;
  margin: 10px 0;
}
.cover-progress {
  margin-top: 12px;
  font-size: 12px;
  color: var(--text-secondary);
}
.cover-progress > div {
  /* Keep numbers clear of overlay scrollbars in a short dialog. */
  padding-right: 8px;
  display: flex;
  justify-content: space-between;
  margin-bottom: 4px;
  font-variant-numeric: tabular-nums;
}
progress {
  display: block;
  width: 100%;
  height: 6px;
  accent-color: var(--accent, #ed4053);
}
.cover-latest {
  font-size: 13px;
  background: var(--surface-alt);
  padding: 10px;
  border-radius: 8px;
  overflow-wrap: anywhere;
}
footer {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  flex-shrink: 0;
  margin-top: 32px;
}
footer button {
  /* Both actions include the same border width, keeping the 175px columns equal. */
  border: 1px solid transparent;
  min-height: 40px;
  min-width: 0;
  padding: 0 16px;
  flex: 1;
  border-radius: 8px;
  font-size: 14px;
}
.cover-cancel {
  border: 1px solid var(--line);
}
.cover-cancel:hover:not(:disabled) {
  background: var(--surface-hover);
}
.primary {
  background: var(--accent-fill);
  color: var(--on-accent);
}
button:disabled {
  opacity: 0.5;
  cursor: default;
}
@media (max-height: 600px) {
  .cover-empty {
    /* An unavailable preview needs room for its recovery actions. Keep the
       actual image/crop viewport at its original size. */
    height: 140px;
  }
  .cover-empty img {
    width: 80px;
    height: 80px;
  }
  .cover-zoom {
    /* Keep the entire 18px thumb inside the initial 296px body viewport. */
    margin-top: 18px;
  }
}
</style>
