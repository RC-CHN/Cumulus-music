<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { useProfileBlacklist } from '../../composables/useProfileBlacklist';
import { useVirtualRows } from '../../composables/useVirtualRows';
import type { AccountView } from '../../services/contracts';
import type {
  ProfileBlacklistEntry,
  ProfileBlacklistResult,
} from '../../services/profileBlacklist';
import Icon from '../Icon.vue';

const props = defineProps<{ account: AccountView }>();
const emit = defineEmits<{
  back: [];
  open: [id: string];
  updated: [id: string, result: ProfileBlacklistResult];
  expired: [];
}>();
const model = useProfileBlacklist(
  () => (props.account.status === 'authenticated' ? props.account.epoch : undefined),
  () => emit('expired'),
);
const columns = ref(2);
const users = computed(() => model.list.value?.users ?? []);
const rowHeight = 100;
const { container, start, end, onScroll } = useVirtualRows(
  () => Math.ceil(users.value.length / columns.value),
  rowHeight,
  2,
);
const visible = computed(() =>
  users.value.slice(start.value * columns.value, end.value * columns.value),
);
const heading = ref<HTMLElement>();
const dialog = ref<HTMLDialogElement>();
const selected = ref<ProfileBlacklistEntry>();
const cancel = ref<HTMLButtonElement>();
const checkPending = ref<HTMLButtonElement>();
const broken = ref(new Set<string>());
let origin: HTMLButtonElement | undefined;
let observer: ResizeObserver | undefined;
watch(
  container,
  (element) => {
    observer?.disconnect();
    if (!element) return;
    observer = new ResizeObserver(([entry]) => {
      const width = entry?.contentRect.width ?? 0;
      columns.value = width >= 1050 ? 4 : width >= 790 ? 3 : width >= 510 ? 2 : 1;
    });
    observer.observe(element);
  },
  { flush: 'post' },
);
onUnmounted(() => observer?.disconnect());

async function ask(entry: ProfileBlacklistEntry, event: MouseEvent) {
  if (
    model.busy.value ||
    model.list.value?.pending ||
    !users.value.some(({ id }) => id === entry.id)
  )
    return;
  selected.value = entry;
  origin = event.currentTarget as HTMLButtonElement;
  await nextTick();
  dialog.value?.showModal();
  cancel.value?.focus({ preventScroll: true });
}
function close(restore = true) {
  dialog.value?.close();
  selected.value = undefined;
  if (restore) (origin?.isConnected ? origin : heading.value)?.focus({ preventScroll: true });
}
async function remove() {
  const entry = selected.value;
  if (!entry || !dialog.value?.open) return;
  close(false);
  const result = await model.act(entry.id, { kind: 'change', expected: true, origin: 'list' });
  if (result) emit('updated', entry.id, result);
  await nextTick();
  (checkPending.value ?? (origin?.isConnected && !origin.disabled ? origin : heading.value))?.focus(
    {
      preventScroll: true,
    },
  );
}
async function reconcile() {
  const pending = model.list.value?.pending;
  if (!pending) return;
  const result = await model.act(pending.id, { kind: 'reconcile' });
  if (result) emit('updated', pending.id, result);
}
watch(
  () => [props.account.epoch, props.account.status],
  () => {
    close(false);
    broken.value = new Set();
    void model.load();
  },
  { flush: 'post' },
);
onMounted(() => {
  heading.value?.focus({ preventScroll: true });
  void model.load();
});
</script>

<template>
  <section
    class="profile-blacklist-page"
    data-user-blacklist
    :aria-busy="model.busy.value"
    aria-label="用户黑名单"
  >
    <header>
      <button class="icon-button" type="button" aria-label="返回设置" @click="emit('back')">
        <Icon name="back" />
      </button>
      <h1 ref="heading" tabindex="-1">{{ account.nickname || '我' }}的黑名单</h1>
      <button
        class="text-button"
        type="button"
        :disabled="model.busy.value || account.status !== 'authenticated'"
        aria-label="刷新黑名单"
        @click="model.load"
      >
        <Icon name="refresh" />刷新
      </button>
    </header>
    <p v-if="model.issue.value" role="alert" class="error-state">
      {{ model.issue.value }}
      <button class="text-button" type="button" :disabled="model.busy.value" @click="model.load">
        重试
      </button>
    </p>
    <p v-if="model.notice.value" role="status" class="muted">{{ model.notice.value }}</p>
    <p v-if="model.list.value?.pending" class="blacklist-pending" role="status">
      有一个操作尚未确认。
      <button
        ref="checkPending"
        class="text-button"
        type="button"
        :disabled="model.busy.value"
        @click="reconcile"
      >
        核对黑名单
      </button>
    </p>
    <div
      ref="container"
      class="blacklist-scroll"
      tabindex="0"
      aria-label="黑名单用户"
      @scroll.passive="onScroll"
    >
      <p v-if="account.status !== 'authenticated'" class="empty-state" role="status">
        登录后可查看自己的黑名单
      </p>
      <p v-else-if="!model.list.value && model.busy.value" class="empty-state" role="status">
        正在读取黑名单…
      </p>
      <p v-else-if="model.list.value && !users.length" class="empty-state">黑名单中暂无用户</p>
      <div :style="{ height: start * rowHeight + 'px' }" aria-hidden="true" />
      <div
        class="blacklist-grid"
        :style="{ gridTemplateColumns: 'repeat(' + columns + ', minmax(0, 1fr))' }"
      >
        <article
          v-for="entry in visible"
          :key="entry.id"
          :data-blacklist-user="entry.id"
          class="blacklist-user"
        >
          <button
            class="blacklist-profile text-button"
            type="button"
            :disabled="entry.closed"
            @click="emit('open', entry.id)"
          >
            <img
              v-if="entry.avatarUrl && !broken.has(entry.id)"
              :src="entry.avatarUrl"
              alt=""
              referrerpolicy="no-referrer"
              @error="broken.add(entry.id)"
            />
            <Icon v-else name="user" />
            <span>
              <strong :title="entry.nickname">{{ entry.nickname }}</strong>
              <small :title="entry.signature">{{
                entry.closed ? '该用户已注销' : entry.signature
              }}</small>
            </span>
          </button>
          <button
            class="secondary"
            type="button"
            :aria-label="'解除 ' + entry.nickname"
            :disabled="model.busy.value || !!model.list.value?.pending"
            data-blacklist-remove
            @click="ask(entry, $event)"
          >
            解除
          </button>
        </article>
      </div>
      <div
        :style="{ height: Math.max(0, Math.ceil(users.length / columns) - end) * rowHeight + 'px' }"
        aria-hidden="true"
      />
    </div>
    <Teleport to="body">
      <dialog
        ref="dialog"
        class="profile-blacklist-confirm"
        aria-labelledby="blacklist-confirm-title"
        @cancel.prevent="close()"
      >
        <header>
          <h2 id="blacklist-confirm-title">确定从黑名单中移除该用户</h2>
          <button class="icon-button" type="button" aria-label="关闭确认" @click="close()">
            <Icon name="close" />
          </button>
        </header>
        <p v-if="selected">{{ selected.nickname }}</p>
        <footer>
          <button ref="cancel" class="secondary" type="button" @click="close()">取消</button>
          <button class="primary" type="button" @click="remove">确定</button>
        </footer>
      </dialog>
    </Teleport>
  </section>
</template>

<style scoped>
.profile-blacklist-page {
  width: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  flex: 1;
}
.profile-blacklist-page > header,
.profile-blacklist-confirm > header,
.profile-blacklist-confirm > footer {
  display: flex;
  align-items: center;
  gap: 12px;
}
.profile-blacklist-page h1 {
  flex: 1;
}
.blacklist-scroll {
  flex: 1;
  min-height: 0;
  overflow: auto;
}
.blacklist-grid {
  display: grid;
  gap: 0 18px;
}
.blacklist-user {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 100px;
  gap: 8px;
  border-bottom: 1px solid var(--line);
  padding-inline: 6px;
}
.blacklist-profile {
  display: flex;
  text-align: left;
  min-width: 0;
  gap: 10px;
  padding: 0;
}
.blacklist-profile > :first-child {
  width: 42px;
  height: 42px;
  flex: 0 0 42px;
  border-radius: 50%;
  object-fit: cover;
  background: var(--surface-alt);
}
.blacklist-profile span,
.blacklist-profile strong,
.blacklist-profile small {
  display: block;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.blacklist-profile small {
  color: var(--muted);
}
.blacklist-user > .secondary {
  flex: none;
}
.blacklist-pending {
  color: var(--muted);
}
.profile-blacklist-confirm {
  width: min(390px, calc(100vw - 32px));
  border: 1px solid var(--line);
  padding: 20px;
  border-radius: 12px;
  color: var(--text);
  background: var(--surface);
}
.profile-blacklist-confirm::backdrop {
  background: var(--overlay);
}
.profile-blacklist-confirm header {
  justify-content: space-between;
}
.profile-blacklist-confirm h2 {
  font-size: 18px;
}
.profile-blacklist-confirm footer {
  justify-content: end;
  margin-top: 18px;
}
</style>
