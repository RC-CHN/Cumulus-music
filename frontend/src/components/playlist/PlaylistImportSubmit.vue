<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useCore } from '../../services/core';
import type {
  ExternalImportAction,
  ExternalImportAuthAction,
  ExternalImportAuthState,
  ExternalImportAuthView,
  ExternalImportDestination,
  ExternalImportInput,
  ExternalImportView,
} from '../../services/externalPlaylistImport';

const props = defineProps<{ epoch: string; input: ExternalImportInput; ready: boolean }>();
const emit = defineEmits<{ authorizationPanel: [shown: boolean]; clearText: [] }>();
const core = useCore();
const value = ref<ExternalImportView>();
const auth = ref<ExternalImportAuthView>();
const busy = ref(false);
const authBusy = ref(false);
const authShown = ref(false);
const error = ref('');
const cancelButton = ref<HTMLButtonElement>();
const proceedButton = ref<HTMLButtonElement>();
const destination = ref<ExternalImportDestination>('new_playlist');
let timer: ReturnType<typeof setTimeout> | undefined;
let alive = true;
let inputVersion = 0;
const canSubmit = computed(() => value.value?.outcome.state !== 'unknown' && props.ready);
const authAvailable = computed(() => value.value?.authorized === true);
const resultDestination = computed(() =>
  value.value?.outcome.state !== 'ready' && value.value?.outcome.destination === 'favorite_songs'
    ? '我喜欢的音乐'
    : '创建的歌单',
);
watch(
  () => authShown.value && !authAvailable.value,
  (shown) => emit('authorizationPanel', shown),
  { flush: 'sync' },
);
const qr = computed(() => {
  const matrix = auth.value?.modules ?? [];
  const width = matrix.length;
  if (width < 21 || width > 177 || matrix.some((row) => row.length !== width)) return;
  return {
    size: width + 8,
    cells: matrix
      .map((row, y) => row.map((cell, x) => (cell ? `M${x + 4} ${y + 4}h1v1h-1z` : '')).join(''))
      .join(''),
  };
});
const authMessage: Record<ExternalImportAuthState, string> = {
  idle: '请用网易云音乐 App 扫码，确认使用当前登录的账号。',
  creating: '正在生成导入授权二维码…',
  waiting_scan: '请用网易云音乐 App 扫码，确认使用当前登录的账号。',
  waiting_confirmation: '已扫码，请在网易云音乐 App 确认。',
  identity_pending: '扫码结果待核对，请确认是否为当前账号。',
  authorized: '当前账号已授权导入。',
  expired: '二维码已过期。',
  challenge: '服务端要求额外验证，未取得导入授权。',
  wrong_account: '扫码账号与当前账号不同，未取得授权。',
  unconfirmed: '未能确认授权。',
};
const canBegin = computed(
  () =>
    !authBusy.value &&
    ['idle', 'expired', 'challenge', 'wrong_account', 'unconfirmed'].includes(
      auth.value?.state ?? 'idle',
    ),
);
function clearPoll() {
  if (timer !== undefined) clearTimeout(timer);
  timer = undefined;
}
function schedule() {
  clearPoll();
  const snapshot = auth.value;
  if (
    !alive ||
    !snapshot?.operationId ||
    !['waiting_scan', 'waiting_confirmation'].includes(snapshot.state) ||
    snapshot.pollAfterMs === null
  )
    return;
  const operation_id = snapshot.operationId;
  timer = setTimeout(
    () => void authorize({ kind: 'poll', operation_id }),
    Math.min(10000, Math.max(1000, snapshot.pollAfterMs)),
  );
}
async function authorize(action: ExternalImportAuthAction) {
  if (!alive || authBusy.value) return;
  clearPoll();
  const epoch = props.epoch;
  authShown.value = true;
  authBusy.value = true;
  error.value = '';
  try {
    const result = await core.externalPlaylistImportAuthAction(epoch, action);
    if (!alive || props.epoch !== epoch) {
      if (action.kind === 'begin' && result.operationId && result.state !== 'authorized')
        void core
          .externalPlaylistImportAuthAction(epoch, {
            kind: 'cancel',
            operation_id: result.operationId,
          })
          .catch(() => {});
      return;
    }
    if (result.epoch !== epoch) return;
    auth.value = result;
    if (result.state === 'authorized') await command({ kind: 'inspect' });
    else schedule();
  } catch {
    if (alive && props.epoch === epoch) error.value = '导入授权暂不可用，请重新打开后核对。';
  } finally {
    if (alive && props.epoch === epoch) authBusy.value = false;
  }
}

async function command(action: ExternalImportAction) {
  if (!alive || busy.value) return;
  const epoch = props.epoch;
  const attempt = inputVersion;
  const preparing = action.kind === 'prepare' || action.kind === 'prepare_text';
  const submittingText = action.kind === 'submit' && value.value?.prepared?.source.kind === 'text';
  busy.value = true;
  error.value = '';
  if (action.kind === 'submit' && value.value) value.value = { ...value.value, prepared: null };
  try {
    const result = await core.externalPlaylistImport(epoch, action);
    const prepared = result.value.prepared;
    if (
      !alive ||
      props.epoch !== epoch ||
      result.epoch !== epoch ||
      (preparing && attempt !== inputVersion)
    ) {
      if (prepared)
        void core
          .externalPlaylistImport(epoch, { kind: 'cancel', receipt: prepared.receipt })
          .catch(() => {});
      return;
    }
    value.value = result.value;
    if (submittingText && attempt === inputVersion) emit('clearText');
    if (prepared) {
      await nextTick();
      if (alive && value.value?.prepared?.receipt === prepared.receipt) {
        const button = cancelButton.value;
        button?.focus({ preventScroll: true });
        button?.scrollIntoView({ block: 'nearest' });
        const dialog = button?.closest('dialog');
        const footer = dialog?.querySelector('footer');
        if (dialog && footer && button) {
          // Some WebViews ignore scroll padding when the footer is sticky.
          const covered =
            button.getBoundingClientRect().bottom - footer.getBoundingClientRect().top;
          if (covered > -8) dialog.scrollTop += covered + 8;
        }
      }
    }
  } catch {
    if (alive && props.epoch === epoch) {
      error.value =
        action.kind === 'submit'
          ? '暂未能读取导入结果。请核对本机回执，勿重复提交。'
          : preparing
            ? '无法准备导入，请检查输入和授权。'
            : '';
      if (action.kind === 'submit') {
        // Fence controls until an explicit local read; never replay the write.
        value.value = undefined;
      }
      if (preparing) {
        void core
          .externalPlaylistImportAuth(epoch)
          .then((next) => {
            if (!alive || props.epoch !== epoch || next.epoch !== epoch) return;
            auth.value = next;
            if (next.state !== 'authorized' && value.value) {
              value.value = { ...value.value, authorized: false };
              authShown.value = false;
            }
          })
          .catch(() => {});
      }
    }
  } finally {
    if (alive && props.epoch === epoch) busy.value = false;
  }
}

function discard(restoreFocus = false) {
  const draft = value.value?.prepared;
  if (draft && value.value) {
    value.value = { ...value.value, prepared: null };
    void core
      .externalPlaylistImport(props.epoch, { kind: 'cancel', receipt: draft.receipt })
      .catch(() => {});
    if (restoreFocus) void nextTick(() => proceedButton.value?.focus());
  }
}
function proceed() {
  if (!value.value || !canSubmit.value || busy.value || authBusy.value) return;
  if (!authAvailable.value) {
    authShown.value = true;
    if (canBegin.value) void authorize({ kind: 'begin' });
    else schedule();
  } else if (props.input.kind === 'text') {
    void command({ kind: 'prepare_text', text: props.input.text, destination: destination.value });
  } else
    void command({
      kind: 'prepare',
      inputs: [...props.input.inputs],
      destination: destination.value,
    });
}

watch(
  [
    destination,
    () => (props.input.kind === 'links' ? props.input.inputs.slice() : props.input.text),
  ],
  () => {
    inputVersion++;
    discard();
  },
  { flush: 'sync' },
);
onMounted(() => {
  const epoch = props.epoch;
  void command({ kind: 'inspect' });
  void core
    .externalPlaylistImportAuth(epoch)
    .then((result) => {
      if (
        alive &&
        props.epoch === epoch &&
        result.epoch === epoch &&
        !auth.value &&
        !authBusy.value
      )
        auth.value = result;
    })
    .catch(() => {});
});
onBeforeUnmount(() => {
  alive = false;
  clearPoll();
  discard();
  if (
    auth.value?.operationId &&
    ['creating', 'waiting_scan', 'waiting_confirmation', 'identity_pending'].includes(
      auth.value.state,
    )
  )
    void core
      .externalPlaylistImportAuthAction(props.epoch, {
        kind: 'cancel',
        operation_id: auth.value.operationId,
      })
      .catch(() => {});
});
</script>

<template>
  <section class="external-import-submit" aria-label="外部歌单导入">
    <p v-if="error" role="alert">{{ error }}</p>
    <p v-if="value?.outcome.state === 'unknown'" role="status" data-import-state="unknown">
      导入结果未知。请在「{{ resultDestination }}」和小秘书私信查看；此客户端不会重发。
    </p>
    <p
      v-if="value?.outcome.state === 'accepted' && !value.prepared"
      role="status"
      data-import-state="accepted"
    >
      任务已受理，可在「{{ resultDestination }}」和小秘书私信查看结果。
    </p>
    <button
      v-if="value?.outcome.state === 'unknown' || (!value && !busy)"
      class="text-button"
      type="button"
      :disabled="busy"
      @click="command({ kind: 'inspect' })"
    >
      核对本机回执
    </button>
    <div v-if="authShown && !authAvailable" class="external-import-auth">
      <svg
        v-if="qr && auth?.state === 'waiting_scan'"
        role="img"
        aria-label="导入授权二维码"
        :viewBox="`0 0 ${qr.size} ${qr.size}`"
        shape-rendering="crispEdges"
      >
        <path :d="`M0 0h${qr.size}v${qr.size}H0z`" fill="white" />
        <path :d="qr.cells" fill="black" />
      </svg>
      <p role="status">{{ authMessage[auth?.state ?? 'idle'] }}</p>
      <button
        v-if="auth?.state === 'identity_pending' && auth.operationId"
        type="button"
        class="secondary"
        :disabled="authBusy"
        @click="authorize({ kind: 'reconcile', operation_id: auth.operationId })"
      >
        核对授权
      </button>
      <button
        v-else-if="canBegin"
        type="button"
        class="secondary"
        @click="authorize({ kind: 'begin' })"
      >
        重新扫码
      </button>
    </div>
    <label
      v-if="value && value.outcome.state !== 'unknown' && (!authShown || authAvailable)"
      class="external-import-destination"
    >
      <input
        v-model="destination"
        type="checkbox"
        true-value="favorite_songs"
        false-value="new_playlist"
        :disabled="busy || authBusy"
      />
      导入我喜欢的音乐
    </label>
    <div
      v-if="value?.prepared"
      class="external-import-confirm"
      data-import-state="confirm"
      :data-import-source="value.prepared.source.kind"
      :data-import-destination="value.prepared.destination"
    >
      <p>
        将把{{
          value.prepared.source.kind === 'text' ? '输入的文字' : '已识别的链接'
        }}交给网易云音乐，{{
          value.prepared.destination === 'favorite_songs'
            ? '服务端将尝试把匹配到的歌曲加入「我喜欢的音乐」。'
            : `服务端将尝试创建「${value.prepared.name}」。`
        }}
      </p>
      <button ref="cancelButton" type="button" class="secondary" @click="discard(true)">
        取消提交
      </button>
      <button
        type="button"
        class="primary"
        :disabled="busy"
        @click="command({ kind: 'submit', receipt: value.prepared.receipt })"
      >
        确认提交导入
      </button>
    </div>
    <button
      v-else-if="
        value &&
        value.outcome.state !== 'unknown' &&
        (canSubmit || input.kind === 'text') &&
        (!authShown || authAvailable)
      "
      ref="proceedButton"
      class="secondary"
      type="button"
      :disabled="!canSubmit || busy || authBusy"
      @click="proceed"
    >
      {{ authAvailable ? '提交导入…' : '授权并导入…' }}
    </button>
  </section>
</template>

<style scoped>
.external-import-submit {
  display: grid;
  gap: 8px;
}
.external-import-destination {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 8px;
  width: fit-content;
}
.external-import-destination input {
  width: auto;
  accent-color: var(--accent);
}
.external-import-auth,
.external-import-confirm {
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 12px;
}
.external-import-auth svg {
  display: block;
  width: 152px;
  max-width: 100%;
  margin: 0 auto;
}
.external-import-auth p,
.external-import-confirm p {
  margin: 0 0 8px;
}
.external-import-confirm button + button {
  margin-left: 8px;
}
</style>
