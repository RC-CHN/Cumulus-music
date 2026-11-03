<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import type { AccountAvatarModel } from '../../composables/useAccountAvatar';
import { useControlFocus } from '../../composables/useControlFocus';
import {
  avatarCanAbandon,
  avatarContinue,
  avatarDraft,
  avatarMessage,
  avatarTerminal,
} from '../../services/accountAvatar';
import type { CoverCrop } from '../../services/playlistCover';
import Icon from '../Icon.vue';
import AvatarCrop from './AvatarCrop.vue';

const props = defineProps<{ model: AccountAvatarModel; epoch: string }>();
const { task, image, busy, error, phase, readFailed } = props.model;
const dialog = ref<HTMLDialogElement>();
const heading = ref<HTMLElement>();
const crop = ref<CoverCrop>();
const editable = computed(() => avatarDraft(task.value) && !busy.value && !readFailed.value);
const canPick = computed(
  () =>
    !task.value ||
    avatarDraft(task.value) ||
    (avatarTerminal(task.value) && !task.value.cleanupPending && !task.value.metadataPending),
);
const message = computed(() => (task.value ? avatarMessage(task.value) : ''));
const progress = computed(() => {
  const value = task.value;
  return value?.phase === 'uploading' && value.total > 0
    ? Math.min(value.bytes, value.total)
    : null;
});
const waiting = computed(
  () =>
    ({
      idle: '',
      reading: '正在读取头像编辑进度…',
      picking: '正在选择图片…',
      preview: '正在读取图片预览…',
      saving: '正在处理头像并核对结果…',
      reconciling: '正在核对当前头像…',
      abandoning: '正在放弃本次头像编辑…',
      cleaning: '正在清理图片缓存…',
    })[phase.value],
);
const focus = useControlFocus(
  () => props.epoch,
  () => heading.value,
);
function acceptCurrent() {
  const expectedRevision = task.value?.observedRevision;
  if (expectedRevision) return props.model.command({ kind: 'accept_current', expectedRevision });
}
function submit() {
  const selection = crop.value;
  if (editable.value && image.value && selection)
    return props.model.command({ kind: 'save', crop: selection });
}
onMounted(async () => {
  dialog.value?.showModal();
  await nextTick();
  heading.value?.focus({ preventScroll: true });
});
onBeforeUnmount(() => dialog.value?.close());
</script>

<template>
  <dialog
    ref="dialog"
    class="account-avatar-dialog"
    :data-avatar-phase="task?.phase ?? 'empty'"
    :aria-busy="busy"
    aria-labelledby="account-avatar-title"
    @cancel.prevent="model.dismiss"
  >
    <header>
      <h2 id="account-avatar-title" ref="heading" tabindex="-1">编辑图片</h2>
      <button
        type="button"
        class="icon-button"
        data-testid="account-avatar-close"
        aria-label="关闭头像编辑"
        :disabled="busy"
        @click="model.dismiss"
      >
        <Icon name="close" />
      </button>
    </header>
    <div class="account-avatar-body">
      <AvatarCrop
        v-if="image"
        :key="task?.id"
        v-model="crop"
        :image="image"
        :initial="task?.crop"
        :disabled="!editable"
      />
      <p v-if="image && editable" class="account-avatar-tip">
        拖动图片调整位置，缩放选择要保留的部分。
      </p>
      <p v-if="waiting" role="status">{{ waiting }}</p>
      <p v-else-if="message && !readFailed" role="status">{{ message }}</p>
      <p v-if="!image && !task && !readFailed" class="account-avatar-tip">
        支持 JPEG、PNG、BMP、GIF，不超过 5 MiB，宽高至少 320 像素。
      </p>
      <div
        v-if="task?.phase === 'binding' && task.observedRevision && !readFailed"
        class="account-avatar-observed"
      >
        <img
          v-if="task.observedAvatarUrl"
          :src="task.observedAvatarUrl"
          alt="核对到的当前头像"
          referrerpolicy="no-referrer"
        />
        <p>{{ task.observedAvatarUrl ? '以上是核对到的当前头像。' : '核对结果未提供头像。' }}</p>
        <button
          type="button"
          class="secondary"
          data-testid="account-avatar-accept-current"
          :disabled="busy"
          @click="focus.run($event, acceptCurrent)"
        >
          采用当前头像
        </button>
      </div>
      <div v-if="progress !== null" class="account-avatar-progress">
        <label for="account-avatar-progress">上传进度</label>
        <progress id="account-avatar-progress" :value="progress" :max="task?.total" />
      </div>
      <p v-if="error" role="alert" class="account-avatar-error">{{ error }}</p>
      <p v-if="task?.mayLeaveObject && task.phase !== 'confirmed'" class="account-avatar-tip">
        图片可能已上传。放弃本次修改不会删除服务端的图片。
      </p>
      <p v-if="task?.cleanupPending" class="account-avatar-tip">暂未清理本地图片缓存。</p>
      <p v-if="task?.metadataPending" class="account-avatar-tip">
        头像已核对，暂未更新本机账号摘要。
      </p>
      <div class="account-avatar-secondary">
        <button
          v-if="readFailed"
          type="button"
          class="text-button"
          data-testid="account-avatar-reload"
          :disabled="busy"
          @click="focus.run($event, model.reload)"
        >
          重新读取头像编辑进度
        </button>
        <button
          v-if="canPick && !readFailed"
          type="button"
          class="text-button"
          data-testid="account-avatar-pick"
          :disabled="busy"
          @click="focus.run($event, model.pick)"
        >
          {{ task && avatarDraft(task) ? '重新选取图片' : '选择图片' }}
        </button>
        <button
          v-if="task && avatarDraft(task) && !image && !readFailed"
          type="button"
          class="text-button"
          data-testid="account-avatar-preview"
          :disabled="busy"
          @click="focus.run($event, model.preview)"
        >
          重新读取预览
        </button>
        <button
          v-if="task && avatarCanAbandon(task) && !avatarDraft(task) && !readFailed"
          type="button"
          class="text-button"
          data-testid="account-avatar-abandon"
          :disabled="busy"
          @click="focus.run($event, () => model.command({ kind: 'abandon' }))"
        >
          放弃本次修改
        </button>
        <button
          v-if="(task?.cleanupPending || task?.metadataPending) && !readFailed"
          type="button"
          class="text-button"
          data-testid="account-avatar-cleanup"
          :disabled="busy"
          @click="focus.run($event, () => model.command({ kind: 'cleanup' }))"
        >
          {{ task?.metadataPending ? '重试本地收尾' : '清理本地缓存' }}
        </button>
      </div>
    </div>
    <footer>
      <button type="button" class="secondary" :disabled="busy" @click="model.dismiss">
        {{
          !task || (avatarDraft(task) && !readFailed)
            ? '取消'
            : avatarTerminal(task)
              ? '完成'
              : '稍后处理'
        }}
      </button>
      <button
        v-if="task && avatarDraft(task) && !readFailed"
        type="button"
        class="primary"
        data-testid="account-avatar-save"
        :disabled="!editable || !image || !crop"
        @click="focus.run($event, submit)"
      >
        保存
      </button>
      <button
        v-else-if="task?.phase === 'binding' && !readFailed"
        type="button"
        class="primary"
        data-testid="account-avatar-reconcile"
        :disabled="busy"
        @click="focus.run($event, () => model.command({ kind: 'reconcile' }))"
      >
        核对当前头像
      </button>
      <button
        v-else-if="task && avatarContinue(task) && !readFailed"
        type="button"
        class="primary"
        data-testid="account-avatar-continue"
        :disabled="busy"
        @click="focus.run($event, () => model.command({ kind: 'continue' }))"
      >
        继续处理
      </button>
    </footer>
  </dialog>
</template>

<style scoped>
.account-avatar-dialog {
  box-sizing: border-box;
  width: 442px;
  max-width: calc(100vw - 24px);
  max-height: calc(100vh - 24px);
  padding: clamp(18px, 5vw, 40px);
  overflow: hidden;
  border: 0;
  border-radius: 16px;
  background: var(--surface-raised);
  color: var(--text);
  box-shadow: 0 18px 70px #0004;
}
.account-avatar-dialog[open] {
  display: flex;
  flex-direction: column;
}
.account-avatar-dialog::backdrop {
  background: transparent;
}
header {
  position: relative;
  flex: none;
  text-align: center;
}
h2 {
  margin: 0;
  font-size: 20px;
  font-weight: 600;
  line-height: 28px;
}
header button {
  position: absolute;
  top: -8px;
  right: -8px;
}
.account-avatar-body {
  flex: 1 1 auto;
  min-height: 0;
  margin-top: 24px;
  padding: 2px;
  overflow: auto;
  scrollbar-width: thin;
  overscroll-behavior: contain;
}
.account-avatar-body p {
  margin: 12px 0;
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 20px;
}
.account-avatar-body .account-avatar-error {
  color: var(--danger);
}
.account-avatar-secondary,
footer {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 12px;
}
.account-avatar-observed {
  margin: 12px 0;
}
.account-avatar-observed img {
  display: block;
  width: 72px;
  height: 72px;
  object-fit: cover;
  border-radius: 50%;
}
.account-avatar-progress {
  display: grid;
  gap: 4px;
  font-size: 13px;
}
.account-avatar-progress progress {
  width: 100%;
}
footer {
  flex: none;
  justify-content: center;
  margin-top: 20px;
}
footer button {
  min-width: 90px;
}
</style>
