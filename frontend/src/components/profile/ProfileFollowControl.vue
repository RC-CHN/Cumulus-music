<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue';
import type { useProfileFollow } from '../../composables/useProfileFollow';
import Icon from '../Icon.vue';

const props = defineProps<{ model: ReturnType<typeof useProfileFollow> }>();
const dialog = ref<HTMLDialogElement>();
const trigger = ref<HTMLButtonElement>();

function dismiss() {
  dialog.value?.close();
  trigger.value?.focus({ preventScroll: true });
}
function press() {
  const shown = props.model.active.value;
  if (!shown?.relation || shown.pending || props.model.busy.value) return;
  if (shown.relation.followed) {
    if (!dialog.value?.open) dialog.value?.showModal();
  } else void props.model.act({ kind: 'change', expected: false });
}
function confirm() {
  if (!dialog.value?.open) return;
  dismiss();
  void props.model.act({ kind: 'change', expected: true });
}
watch(
  () => props.model.active.value,
  () => {
    if (dialog.value?.open) dismiss();
  },
);
onBeforeUnmount(() => dialog.value?.close());
</script>

<template>
  <div v-if="model.active.value || model.message.value" class="profile-follow" data-profile-follow>
    <button
      v-if="model.active.value?.relation"
      ref="trigger"
      class="profile-follow-button"
      :class="model.active.value.relation.followed ? 'secondary' : 'primary'"
      :aria-busy="model.busy.value"
      :disabled="model.busy.value || model.active.value.pending"
      type="button"
      @click="press"
    >
      <Icon :name="model.active.value.relation.followed ? 'check' : 'plus'" />
      {{
        model.active.value.relation.mutual
          ? '已互关'
          : model.active.value.relation.followed
            ? '已关注'
            : '关注'
      }}
    </button>
    <button
      v-if="model.active.value?.pending"
      class="text-button"
      type="button"
      :disabled="model.busy.value"
      @click="model.act({ kind: 'reconcile' })"
    >
      核对关注状态
    </button>
    <span v-if="model.message.value" class="muted" role="status">{{ model.message.value }}</span>
    <Teleport to="body">
      <dialog
        ref="dialog"
        class="profile-unfollow-dialog"
        aria-label="确定取消关注该好友?"
        @cancel.prevent="dismiss"
      >
        <header>
          <h2>确定取消关注该好友?</h2>
          <button class="icon-button" type="button" aria-label="关闭确认" @click="dismiss">
            <Icon name="close" />
          </button>
        </header>
        <footer>
          <button class="secondary" type="button" @click="confirm">仍要取消</button>
          <button autofocus class="primary" type="button" @click="dismiss">继续关注</button>
        </footer>
      </dialog>
    </Teleport>
  </div>
</template>

<style scoped>
.profile-follow,
.profile-follow-button,
.profile-unfollow-dialog header,
.profile-unfollow-dialog footer {
  display: flex;
  gap: 10px;
  align-items: center;
}
.profile-follow {
  margin-top: 12px;
  flex-wrap: wrap;
}
.profile-follow-button {
  gap: 6px;
}
.profile-follow-button :deep(svg) {
  width: 16px;
  height: 16px;
}
.profile-unfollow-dialog {
  width: min(390px, calc(100vw - 32px));
  padding: 20px;
  border: 1px solid var(--line);
  border-radius: 12px;
  color: var(--text);
  background: var(--surface);
}
.profile-unfollow-dialog::backdrop {
  background: transparent;
}
.profile-unfollow-dialog header {
  justify-content: space-between;
}
.profile-unfollow-dialog h2 {
  font-size: 18px;
}
.profile-unfollow-dialog footer {
  justify-content: end;
  margin-top: 18px;
}
</style>
