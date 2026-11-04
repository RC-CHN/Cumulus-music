<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, shallowRef, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import { useCore } from '../../services/core';
import type { MemberWelfareView } from '../../services/memberWelfare';
import type { MenuItem } from '../../services/menu';
import ActionMenu from '../ActionMenu.vue';
import Icon from '../Icon.vue';
import ModalTransition from '../ModalTransition.vue';
import MemberWelfareDialog from '../member/MemberWelfareDialog.vue';

const props = defineProps<{ account: AccountView; disabled: boolean }>();
const emit = defineEmits<{ profile: []; accounts: []; login: []; member: [] }>();
const core = useCore();
const signedIn = computed(() => props.account.status === 'authenticated');
const profile = computed(() => signedIn.value && !!props.account.profileShortcut);
const label = computed(() =>
  signedIn.value ? (props.account.nickname ?? '我的账号') : '点击登录',
);
const avatarFailed = ref(false);
watch([() => props.account.epoch, () => props.account.avatarUrl], () => {
  avatarFailed.value = false;
});
const showAvatar = computed(() => props.account.avatarUrl && !avatarFailed.value);
const nickname = ref<HTMLButtonElement>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const menuOpen = ref(false);
const assigned = ref(false);
const preview = shallowRef<MemberWelfareView>();
const selected = shallowRef<MemberWelfareView>();
let menuRead = 0;
const items = computed<MenuItem[]>(() => [
  { key: 'member', label: '会员中心', icon: 'star' },
  ...(preview.value
    ? [
        {
          key: 'welfare',
          label: '福利活动中心',
          icon: 'star',
          hint:
            [
              preview.value.rightTips,
              preview.value.unreadCount ? `${preview.value.unreadCount} 条未读` : null,
            ]
              .filter(Boolean)
              .join(' · ') || undefined,
        },
      ]
    : []),
  { key: 'accounts', label: '切换账号', icon: 'user' },
]);
function closed() {
  menuOpen.value = false;
  const revision = ++menuRead;
  // ActionMenu closes before emitting its selected item. Keep that exact
  // snapshot just long enough to open the child dialog from the same visit.
  void nextTick(() => {
    if (revision === menuRead) preview.value = undefined;
  });
}
watch(
  [() => props.account.epoch, signedIn],
  ([epoch, active], _, onCleanup) => {
    assigned.value = false;
    selected.value = undefined;
    preview.value = undefined;
    ++menuRead;
    menu.value?.close();
    if (!active) return;
    let current = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    onCleanup(() => {
      current = false;
      clearTimeout(timer);
    });
    async function refresh() {
      try {
        const reply = await core.memberWelfareAvailable(epoch);
        if (current) assigned.value = reply.epoch === epoch && reply.value === true;
      } catch {
        if (current) assigned.value = false;
      } finally {
        if (current && !assigned.value) menu.value?.close(true);
        if (current) timer = setTimeout(() => void refresh(), 30 * 60 * 1000);
      }
    }
    void refresh();
  },
  { immediate: true, flush: 'sync' },
);
watch(
  () => props.disabled,
  (disabled) => {
    if (disabled) menu.value?.close();
  },
);
onUnmounted(() => {
  ++menuRead;
});
function show() {
  if (!nickname.value || props.disabled || !signedIn.value || !assigned.value || menuOpen.value)
    return;
  preview.value = undefined;
  const epoch = props.account.epoch;
  // showBelow closes any old popup synchronously before opening the new one.
  void menu.value?.showBelow(nickname.value);
  const revision = ++menuRead;
  void core
    .memberWelfareList(epoch)
    .then((reply) => {
      if (
        menuOpen.value &&
        revision === menuRead &&
        assigned.value &&
        signedIn.value &&
        props.account.epoch === epoch &&
        reply.epoch === epoch
      )
        preview.value = reply.value ?? undefined;
    })
    .catch(() => {});
}
function toggle() {
  if (!signedIn.value) emit('login');
  else if (!assigned.value) emit('accounts');
  else if (menuOpen.value) menu.value?.close(true);
  else show();
}
function select(key: string) {
  if (!signedIn.value) return;
  if (key === 'member') emit('member');
  else if (key === 'accounts') emit('accounts');
  else if (key === 'welfare' && assigned.value && preview.value) selected.value = preview.value;
}
</script>

<template>
  <div class="account-control" :class="{ 'account-guest': !signedIn }">
    <button
      v-if="profile"
      class="account-avatar-button"
      aria-label="打开我的主页"
      title="打开我的主页"
      type="button"
      :disabled="disabled"
      @click="emit('profile')"
    >
      <img
        v-if="showAvatar"
        :src="account.avatarUrl!"
        alt=""
        referrerpolicy="no-referrer"
        @error="avatarFailed = true"
      />
      <span v-else class="avatar"><Icon name="user" /></span>
    </button>
    <button
      ref="nickname"
      class="account-button"
      :class="{ 'account-nickname': profile }"
      type="button"
      :title="label"
      :disabled="disabled"
      :aria-haspopup="assigned ? 'menu' : undefined"
      :aria-expanded="assigned ? menuOpen : undefined"
      @click="toggle"
      @keydown.down="assigned && ($event.preventDefault(), show())"
    >
      <template v-if="!profile">
        <img
          v-if="showAvatar"
          :src="account.avatarUrl!"
          alt=""
          referrerpolicy="no-referrer"
          @error="avatarFailed = true"
        />
        <span v-else class="avatar"><Icon name="user" /></span>
      </template>
      <span>{{ label }}</span>
      <Icon v-if="profile" name="down" class="account-unfold" />
    </button>
    <ActionMenu
      v-if="assigned"
      ref="menu"
      context-only
      variant="account"
      label="账号菜单"
      :items="items"
      :disabled="disabled || !assigned"
      @opened="menuOpen = true"
      @closed="closed"
      @select="select"
    >
      <template #item="{ item }">
        <span class="welfare-menu-label">{{ item.label }}</span>
        <template v-if="item.key === 'welfare' && preview">
          <span v-if="preview.unreadCount" class="welfare-menu-count">
            {{ preview.unreadCount }} 条未读
          </span>
          <span v-if="preview.rightTips" class="welfare-menu-tip">{{ preview.rightTips }}</span>
        </template>
      </template>
    </ActionMenu>
    <ModalTransition :return-focus="() => nickname ?? null">
      <MemberWelfareDialog v-if="selected" :view="selected" @close="selected = undefined" />
    </ModalTransition>
  </div>
</template>

<style scoped>
.welfare-menu-label,
.welfare-menu-count {
  flex: none;
  white-space: nowrap;
}
.welfare-menu-count {
  padding: 1px 4px;
  color: var(--accent);
  background: var(--accent-soft);
  border-radius: 5px;
  font-size: 11px;
}
.welfare-menu-tip {
  min-width: 0;
  margin-inline-start: auto;
  color: var(--muted);
  font-size: 12px;
  text-align: right;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
