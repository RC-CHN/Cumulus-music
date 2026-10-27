<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue';
import type { AccountView } from '../services/contracts';
import { failure } from '../services/contracts';
import type { SavedAccount } from '../services/auth';
import { useCore } from '../services/core';
import { useLogin } from '../composables/useLogin';
import { useSavedAccounts } from '../composables/useSavedAccounts';
import Icon from './Icon.vue';
import SessionRenewal from './SessionRenewal.vue';
const props = defineProps<{ account: AccountView }>();
const emit = defineEmits<{ close: []; login: []; changed: [] }>();
const core = useCore();
const model = useLogin(core, () => emit('changed'));
const { busy } = model;
const dialog = ref<HTMLDialogElement>();
const saved = useSavedAccounts(core, props.account.epoch);
const { accounts, loading, removing } = saved;
const error = computed(() => model.error.value || saved.error.value);
const removal = shallowRef<SavedAccount>();
const leaving = ref(false),
  confirm = ref(false),
  remote = ref(props.account.status === 'authenticated');
let alive = true,
  focused: HTMLElement | null = null;
const epoch = props.account.epoch;
watch(
  () => props.account.epoch,
  () => emit('close'),
);
async function load() {
  model.error.value = '';
  await saved.load();
}
async function select(account: SavedAccount) {
  if (busy.value || leaving.value || removing.value || account.current || account.removalPending)
    return;
  saved.error.value = '';
  const flow = await model.start(epoch, 'saved_account', account.id);
  if (flow && alive) await model.run({ kind: 'reconcile' });
}
async function logout() {
  if (busy.value || leaving.value || removing.value) return;
  leaving.value = true;
  saved.error.value = '';
  model.error.value = '';
  try {
    await core.logoutAccount(epoch, remote.value);
    if (alive) emit('changed');
  } catch (e) {
    if (alive) saved.error.value = failure(e).message;
  } finally {
    if (alive) leaving.value = false;
  }
}
async function remove() {
  const target = removal.value;
  if (!target || busy.value || leaving.value || removing.value) return;
  model.error.value = '';
  await saved.remove(target);
  if (alive) removal.value = undefined;
}
onMounted(() => {
  focused = document.activeElement as HTMLElement;
  dialog.value?.showModal();
  void load();
});
onUnmounted(() => {
  alive = false;
  model.dispose();
  saved.dispose();
  focused?.focus();
});
</script>
<template>
  <dialog ref="dialog" class="account-dialog" aria-label="切换账号" @cancel.prevent="emit('close')">
    <button class="icon-button dialog-close" aria-label="关闭账号窗口" @click="emit('close')">
      <Icon name="close" />
    </button>
    <h2>{{ removal ? '确认移除账号' : confirm ? '退出登录' : '切换账号' }}</h2>
    <template v-if="removal">
      <p>{{ removal.nickname }}</p>
      <p class="muted">将移除本机保存的这条登录记录，再次使用需要重新登录。</p>
      <div class="account-confirm">
        <button :disabled="removing" @click="removal = undefined">取消</button>
        <button class="primary" :disabled="removing" @click="remove">
          {{ removing ? '正在移除…' : '确认移除' }}
        </button>
      </div>
    </template>
    <template v-else-if="confirm"
      ><p>{{ account.nickname || '当前账号' }}</p>
      <label class="logout-option"
        ><input v-model="remote" type="checkbox" :disabled="leaving" />同时退出服务器会话</label
      >
      <div class="account-confirm">
        <button :disabled="leaving" @click="confirm = false">取消</button
        ><button class="primary" :disabled="leaving" @click="logout">
          {{ leaving ? '正在退出…' : '确认退出' }}
        </button>
      </div></template
    >
    <template v-else>
      <p v-if="account.status === 'authenticated'" class="muted">
        当前账号：{{ account.nickname }}
      </p>
      <SessionRenewal
        :key="epoch"
        :epoch="epoch"
        :disabled="busy || leaving || removing"
        @login="emit('login')"
      />
      <p v-if="loading" role="status">正在读取账号…</p>
      <p v-else-if="!accounts.length" class="muted">暂无已保存账号</p>
      <div class="saved-account-list">
        <div v-for="item in accounts" :key="item.id" class="saved-account-row">
          <button
            class="saved-account"
            :data-current="item.current"
            :disabled="item.current || busy || leaving || removing || item.removalPending"
            @click="select(item)"
          >
            <img
              v-if="item.avatarUrl"
              :src="item.avatarUrl"
              alt=""
              referrerpolicy="no-referrer"
            /><Icon v-else name="user" /><span
              >{{ item.nickname }}<small v-if="item.current">当前账号</small
              ><small v-else-if="item.removalPending">凭据清理未完成</small
              ><small v-else-if="!item.autoLogin">未开启自动登录</small></span
            >
          </button>
          <button
            v-if="!item.current"
            class="saved-remove"
            :aria-label="`移除账号 ${item.nickname}`"
            :disabled="busy || leaving || removing"
            @click="removal = item"
          >
            {{ item.removalPending ? '继续移除' : '移除' }}
          </button>
        </div>
      </div>
      <button class="account-add" :disabled="busy || leaving || removing" @click="emit('login')">
        ＋ 添加账号
      </button>
      <button
        v-if="account.canSignOut"
        class="account-logout"
        :disabled="busy || leaving || removing"
        @click="confirm = true"
      >
        退出登录
      </button>
    </template>
    <p v-if="busy" role="status">正在核对并切换账号…</p>
    <p v-if="error" class="login-status" role="alert">
      {{ error
      }}<button v-if="!busy && !confirm && !removal && !removing" @click="load">刷新列表</button>
    </p>
  </dialog>
</template>
