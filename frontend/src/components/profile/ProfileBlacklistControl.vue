<script setup lang="ts">
import { nextTick, ref, shallowRef, watch } from 'vue';
import { useProfileBlacklist } from '../../composables/useProfileBlacklist';
import type {
  ProfileBlacklistHeader,
  ProfileBlacklistResult,
} from '../../services/profileBlacklist';
import Icon from '../Icon.vue';

const props = defineProps<{
  epoch?: string;
  uid?: string;
  header?: ProfileBlacklistHeader | null;
}>();
const emit = defineEmits<{
  updated: [uid: string, result: ProfileBlacklistResult];
  expired: [];
}>();
const model = useProfileBlacklist(
  () => props.epoch,
  () => emit('expired'),
);
const shown = shallowRef(props.header);
const container = ref<HTMLElement>();
const trigger = ref<HTMLButtonElement>();
const menuItem = ref<HTMLButtonElement>();
const menu = ref(false);
watch(
  () => [props.uid, props.epoch],
  () => {
    model.reset();
    menu.value = false;
    shown.value = props.header;
  },
  { flush: 'sync' },
);
watch(
  () => props.header,
  (value) => {
    shown.value = value;
  },
  { flush: 'sync' },
);

function closeMenu(restore = false) {
  menu.value = false;
  if (restore) trigger.value?.focus({ preventScroll: true });
}
async function openMenu() {
  if (model.busy.value || shown.value?.pending || shown.value?.busyWithAnother) return;
  if (menu.value) {
    closeMenu(true);
    return;
  }
  menu.value = true;
  await nextTick();
  menuItem.value?.focus({ preventScroll: true });
}
function escape(event: KeyboardEvent) {
  if (!menu.value) return;
  event.preventDefault();
  event.stopPropagation();
  closeMenu(true);
}
function left(event: FocusEvent) {
  if (!(event.relatedTarget instanceof Node) || !container.value?.contains(event.relatedTarget))
    closeMenu();
}
async function change(reconcile = false) {
  const uid = props.uid;
  const current = shown.value;
  if (!uid || !current || (!reconcile && (current.pending || current.busyWithAnother))) return;
  closeMenu(true);
  const result = await model.act(
    uid,
    reconcile
      ? { kind: 'reconcile' }
      : { kind: 'change', expected: current.blocked, origin: 'profile' },
  );
  if (!result || uid !== props.uid) return;
  shown.value = {
    ...current,
    blocked: result.blocked ?? current.blocked,
    canAdd: result.blocked === false ? current.canAdd || current.blocked : current.canAdd,
    pending: result.pending?.id === uid,
    busyWithAnother: !!result.pending && result.pending.id !== uid,
  };
  emit('updated', uid, result);
  await nextTick();
  if (document.activeElement === document.body) trigger.value?.focus({ preventScroll: true });
}
</script>

<template>
  <div
    v-if="shown || model.issue.value || model.notice.value"
    ref="container"
    class="profile-blacklist-control"
    data-profile-blacklist-control
    @focusout="left"
    @keydown.escape="escape"
  >
    <button
      v-if="shown?.blocked"
      ref="trigger"
      class="secondary"
      type="button"
      :disabled="model.busy.value || shown.pending || shown.busyWithAnother"
      data-profile-unblock
      @click="change()"
    >
      解除黑名单
    </button>
    <div v-else-if="shown?.canAdd" class="profile-blacklist-menu">
      <button
        ref="trigger"
        class="icon-button"
        type="button"
        aria-label="更多用户操作"
        aria-haspopup="menu"
        :aria-expanded="menu"
        :disabled="model.busy.value || shown.pending || shown.busyWithAnother"
        data-profile-blacklist-menu
        @click="openMenu"
        @keydown.down.prevent="openMenu"
      >
        <Icon name="more" />
      </button>
      <div v-if="menu" class="profile-blacklist-popup" role="menu" aria-label="用户操作">
        <button ref="menuItem" class="text-button" type="button" role="menuitem" @click="change()">
          加黑名单
        </button>
      </div>
    </div>
    <button
      v-if="shown?.pending"
      class="text-button"
      type="button"
      :disabled="model.busy.value"
      @click="change(true)"
    >
      核对黑名单
    </button>
    <span v-if="shown?.busyWithAnother" class="muted">请在设置中核对尚未确认的黑名单操作</span>
    <span v-if="model.issue.value" role="alert">{{ model.issue.value }}</span>
    <span v-else-if="model.notice.value" role="status">{{ model.notice.value }}</span>
  </div>
</template>

<style scoped>
.profile-blacklist-control {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 8px;
}
.profile-blacklist-menu {
  position: relative;
}
.profile-blacklist-popup {
  position: absolute;
  inset: calc(100% + 4px) auto auto 0;
  z-index: 3;
  min-width: max-content;
  padding: 4px 8px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface);
  box-shadow: var(--shadow);
}
</style>
