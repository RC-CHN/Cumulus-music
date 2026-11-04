<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue';
import type { AccountView } from '../services/contracts';
import { failure } from '../services/contracts';
import type { SavedAccount } from '../services/auth';
import { useCore } from '../services/core';
import { useLogin } from '../composables/useLogin';
import { useQuickLoginEligibility } from '../composables/useQuickLoginEligibility';
import { useServerQuickLogin } from '../composables/useServerQuickLogin';
import ServerQuickLoginPanel from './account/ServerQuickLoginPanel.vue';
import { useSavedAccounts } from '../composables/useSavedAccounts';
import Icon from './Icon.vue';
import SessionRenewal from './SessionRenewal.vue';
import { useConfirmationFocus } from '../composables/useConfirmationFocus';
import { useControlFocus } from '../composables/useControlFocus';
const props = defineProps<{ account: AccountView }>();
const emit = defineEmits<{
  close: [];
  login: [];
  changed: [];
  listeningRank: [];
  profileShortcut: [kind: 'notes' | 'following' | 'followers'];
}>();
const profileCount = new Intl.NumberFormat('zh-CN', { notation: 'compact' });
const profileSections = [
  { kind: 'notes', label: '笔记' },
  { kind: 'following', label: '关注' },
  { kind: 'followers', label: '粉丝' },
] as const;
const core = useCore();
const model = useLogin(core, () => emit('changed'));
const { busy } = model;
const dialog = ref<HTMLDialogElement>();
const saved = useSavedAccounts(core, props.account.epoch);
const { accounts, loading, removing } = saved;
const quick = useQuickLoginEligibility(core, () => props.account.epoch);
const quickTarget = shallowRef<{ id: string; name: string; mode: 'guest' | 'active' }>();
const isQuickGuest = computed(
  () =>
    props.account.status === 'signed_out' ||
    (props.account.status === 'unknown' && props.account.canSignOut === false),
);
const guestEpoch = () => (isQuickGuest.value ? props.account.epoch : null);
const activeEpoch = () => (props.account.status === 'authenticated' ? props.account.epoch : null);
const quickLogin = useServerQuickLogin(core, guestEpoch, () => emit('changed'));
const activeSwitch = useServerQuickLogin(
  {
    beginServerQuickLogin: (epoch, id, consent) => core.beginActiveServerSwitch(epoch, id, consent),
    serverQuickLoginCommand: (epoch, id, command) =>
      core.activeServerSwitchCommand(epoch, id, command),
  },
  activeEpoch,
  () => emit('changed'),
);
const exchange = computed(() => (quickTarget.value?.mode === 'active' ? activeSwitch : quickLogin));
const cancellingQuick = computed(() => exchange.value.cancelling.value);
const { view: eligibility, state: eligibilityState, busy: checkingEligibility } = quick;
const quickRows = computed(() => {
  const rows = eligibility.value?.accounts ?? [];
  const names = new Map(accounts.value.map((item) => [item.id, item.nickname]));
  return rows.flatMap((row) => {
    const name = names.get(row.id) ?? (row.current ? props.account.nickname : undefined);
    return name || row.current ? [{ ...row, name: name || '当前账号' }] : [];
  });
});
const uncheckedCount = computed(() => {
  const checked = new Set(eligibility.value?.accounts.map((row) => row.id));
  return accounts.value.filter((item) => !checked.has(item.id)).length;
});
const error = computed(() => model.error.value || saved.error.value);
const removal = shallowRef<SavedAccount>();
const leaving = ref(false),
  confirm = ref(false),
  remote = ref(props.account.status === 'authenticated');
let alive = true,
  focused: HTMLElement | null = null;
const epoch = props.account.epoch;
const actionFocus = useControlFocus(
  () => props.account.epoch,
  () => dialog.value?.querySelector<HTMLElement>('[data-confirmation-fallback]'),
);
function deactivate() {
  if (!alive) return;
  alive = false;
  model.dispose();
  saved.dispose();
  quick.dispose();
  quickLogin.dispose();
  activeSwitch.dispose();
}
function login() {
  deactivate();
  emit('login');
}
useConfirmationFocus(
  dialog,
  computed(() =>
    quickTarget.value
      ? `quick:${quickTarget.value.id}`
      : (removal.value?.id ?? (confirm.value ? 'logout' : undefined)),
  ),
);
async function leaveQuick(after?: () => void) {
  const flow = exchange.value;
  const started = flow.pending.value || flow.view.value || flow.problem.value;
  if (!quickTarget.value || !(await flow.cancel()) || !alive) return;
  if (started) quick.reset();
  quickTarget.value = undefined;
  after?.();
}
function showQuick(row: { id: string; name: string; eligible: boolean; current: boolean }) {
  if (
    (!guestEpoch() && !activeEpoch()) ||
    !row.eligible ||
    row.current ||
    busy.value ||
    leaving.value ||
    removing.value ||
    checkingEligibility.value
  )
    return;
  if (!accounts.value.some((item) => item.id === row.id && !item.removalPending && !item.current))
    return;
  quickTarget.value = { id: row.id, name: row.name, mode: activeEpoch() ? 'active' : 'guest' };
}
function startQuick() {
  const target = quickTarget.value;
  if (target) return exchange.value.start(target.id, true);
}
function close() {
  if (quickTarget.value) {
    void leaveQuick(close);
    return;
  }
  if (!leaving.value && !removing.value) {
    deactivate();
    emit('close');
  }
}
function cancel() {
  if (leaving.value || removing.value) return;
  if (quickTarget.value) void leaveQuick();
  else if (removal.value) removal.value = undefined;
  else if (confirm.value) confirm.value = false;
  else close();
}
watch(
  [() => props.account.epoch, () => props.account.status],
  () => {
    deactivate();
    emit('close');
  },
  { flush: 'sync' },
);
async function load() {
  quick.reset();
  model.error.value = '';
  await saved.load();
}
async function select(account: SavedAccount) {
  if (busy.value || leaving.value || removing.value || account.current || account.removalPending)
    return;
  quick.reset();
  saved.error.value = '';
  const flow = await model.start(epoch, 'saved_account', account.id);
  if (flow && alive) await model.run({ kind: 'reconcile' });
}
async function logout() {
  if (busy.value || leaving.value || removing.value) return;
  quick.reset();
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
  quick.reset();
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
  focused?.focus();
});
onBeforeUnmount(deactivate);
</script>
<template>
  <dialog
    ref="dialog"
    class="account-dialog"
    aria-labelledby="account-dialog-title"
    @cancel.prevent="cancel"
  >
    <button
      class="icon-button dialog-close"
      aria-label="关闭账号窗口"
      data-confirmation-fallback
      :disabled="leaving || removing || cancellingQuick"
      @click="close"
    >
      <Icon name="close" />
    </button>
    <h2 id="account-dialog-title">
      {{
        quickTarget
          ? quickTarget.mode === 'active'
            ? '服务端切换账号'
            : '服务端快捷登录'
          : removal
            ? '确认移除账号'
            : confirm
              ? '退出登录'
              : '切换账号'
      }}
    </h2>
    <ServerQuickLoginPanel
      v-if="quickTarget"
      :name="quickTarget.name"
      :current-name="account.nickname"
      :mode="quickTarget.mode"
      :model="exchange"
      @consent="startQuick"
      @back="leaveQuick()"
      @alternate="leaveQuick(login)"
    />
    <template v-else-if="removal">
      <p>{{ removal.nickname }}</p>
      <p class="muted">将移除本机保存的这条登录记录，再次使用需要重新登录。</p>
      <div class="account-confirm">
        <button data-confirmation-cancel :disabled="removing" @click="removal = undefined">
          取消
        </button>
        <button class="primary" :disabled="removing" @click="actionFocus.run($event, remove)">
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
        <button data-confirmation-cancel :disabled="leaving" @click="confirm = false">取消</button
        ><button class="primary" :disabled="leaving" @click="actionFocus.run($event, logout)">
          {{ leaving ? '正在退出…' : '确认退出' }}
        </button>
      </div></template
    >
    <template v-else>
      <p v-if="account.status === 'authenticated'" class="muted">
        当前账号：{{ account.nickname }}
      </p>
      <div
        v-if="account.status === 'authenticated' && account.profileShortcut"
        class="account-profile-shortcuts"
        role="group"
        aria-label="我的主页快捷入口"
      >
        <button
          v-for="section in profileSections"
          :key="section.kind"
          class="text-button"
          type="button"
          :disabled="busy || leaving || removing"
          @click="emit('profileShortcut', section.kind)"
        >
          <strong v-if="account.profileShortcut[section.kind] !== null">
            {{ profileCount.format(account.profileShortcut[section.kind]!) }}
          </strong>
          <span>{{ section.label }}</span>
        </button>
      </div>
      <SessionRenewal
        :key="epoch"
        :epoch="epoch"
        :disabled="busy || leaving || removing"
        @login="login"
      />
      <h3 class="account-picker-local-heading">本机保存的账号</h3>
      <p class="muted account-picker-local-help">
        {{
          account.status === 'authenticated'
            ? '选择账号可尝试用本机记录切换。'
            : '选择账号可尝试用本机记录恢复登录。'
        }}
      </p>
      <p v-if="loading" role="status">正在读取账号…</p>
      <p v-else-if="!accounts.length" class="muted">暂无已保存账号</p>
      <div class="saved-account-list">
        <div v-for="item in accounts" :key="item.id" class="saved-account-row">
          <button
            class="saved-account"
            :data-current="item.current"
            :disabled="item.current || busy || leaving || removing || item.removalPending"
            @click="actionFocus.run($event, () => select(item))"
          >
            <img
              v-if="item.avatarUrl"
              :src="item.avatarUrl"
              alt=""
              referrerpolicy="no-referrer"
            /><Icon v-else name="user" /><span
              >{{ item.nickname }}<small v-if="item.current">当前账号</small
              ><small v-else-if="item.removalPending">凭据清理未完成</small
              ><small v-if="!item.autoLogin && !item.removalPending">未开启自动登录</small></span
            >
          </button>
          <button
            v-if="!item.current"
            class="saved-remove"
            :aria-label="`移除账号 ${item.nickname}`"
            :data-confirmation-entry="item.id"
            :disabled="busy || leaving || removing"
            @click="removal = item"
          >
            {{ item.removalPending ? '继续移除' : '移除' }}
          </button>
        </div>
      </div>
      <section
        class="account-quick-eligibility"
        data-testid="quick-login-eligibility"
        :data-quick-eligibility-state="eligibilityState"
        aria-labelledby="account-quick-eligibility-title"
      >
        <h3 id="account-quick-eligibility-title">
          {{ account.status === 'authenticated' ? '服务端切换资格' : '服务端快捷登录资格' }}
        </h3>
        <p id="account-quick-eligibility-help">
          向网易云音乐服务核对最多 3 个本机账号记录。此操作不会切换或移除账号。
        </p>
        <button
          class="secondary"
          type="button"
          data-testid="quick-login-eligibility-check"
          aria-describedby="account-quick-eligibility-help"
          :disabled="loading || busy || leaving || removing || checkingEligibility"
          @click="actionFocus.run($event, quick.check)"
        >
          {{
            checkingEligibility
              ? '正在核对…'
              : eligibilityState === 'ready'
                ? '重新核对'
                : eligibilityState === 'error'
                  ? '重试核对'
                  : account.status === 'authenticated'
                    ? '核对切换资格'
                    : '核对快捷登录资格'
          }}
        </button>
        <p v-if="eligibilityState === 'loading'" role="status">正在向服务核对…</p>
        <p v-else-if="eligibilityState === 'error'" role="alert">
          {{
            account.status === 'authenticated'
              ? '暂时无法核对切换资格，本机记录未改变。'
              : '暂时无法核对快捷登录资格，本机记录未改变。'
          }}
        </p>
        <template v-else-if="eligibilityState === 'ready' && eligibility">
          <p role="status">
            {{
              eligibility.checkedCount === 0
                ? '没有可核对的快捷登录记录。'
                : `已核对 ${eligibility.checkedCount} 个账号。`
            }}
          </p>
          <ul v-if="quickRows.length" aria-label="已核对的账号">
            <li v-for="item in quickRows" :key="item.id" :data-quick-eligible="item.eligible">
              <span
                >{{ item.name
                }}<small v-if="item.current && item.name !== '当前账号'">（当前账号）</small></span
              >
              <span>{{ item.eligible ? '符合条件' : '不符合条件' }}</span>
              <button
                v-if="
                  (isQuickGuest || account.status === 'authenticated') &&
                  item.eligible &&
                  !item.current &&
                  accounts.some((saved) => saved.id === item.id && !saved.removalPending)
                "
                type="button"
                class="secondary"
                :data-testid="
                  account.status === 'authenticated'
                    ? 'active-server-switch-open'
                    : 'server-quick-login-open'
                "
                :data-confirmation-entry="`quick:${item.id}`"
                :disabled="busy || leaving || removing || checkingEligibility"
                :aria-label="`${account.status === 'authenticated' ? '通过服务切换到' : '快捷登录'} ${item.name}`"
                @click="showQuick(item)"
              >
                {{ account.status === 'authenticated' ? '服务端切换' : '快捷登录' }}
              </button>
            </li>
          </ul>
          <p v-if="uncheckedCount">其余本机记录未核对。</p>
        </template>
      </section>
      <button
        v-if="account.status === 'authenticated'"
        class="text-button"
        type="button"
        :disabled="busy || leaving || removing"
        @click="emit('listeningRank')"
      >
        我的听歌排行
      </button>
      <button class="account-add" :disabled="busy || leaving || removing" @click="login">
        ＋ 添加账号
      </button>
      <button
        v-if="account.canSignOut"
        class="account-logout"
        data-confirmation-entry="logout"
        :disabled="busy || leaving || removing"
        @click="confirm = true"
      >
        退出登录
      </button>
    </template>
    <p v-if="busy" role="status">正在核对并切换账号…</p>
    <p v-if="error && !quickTarget" class="login-status" role="alert">
      {{ error
      }}<button v-if="!busy && !confirm && !removal && !removing" @click="load">刷新列表</button>
    </p>
  </dialog>
</template>

<style scoped>
.account-profile-shortcuts {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 4px;
  margin-block: 0.75rem;
}
.account-profile-shortcuts button {
  display: grid;
  justify-items: center;
  gap: 0.2rem;
}
.account-profile-shortcuts strong {
  color: var(--text);
  font-variant-numeric: tabular-nums;
}
</style>
