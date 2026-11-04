<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useId, watch } from 'vue';
import { failure, type AccountData } from '../services/contracts';
import {
  maxReportDescriptionBytes,
  type ReportChoice,
  type ReportCommand,
  type ReportReason,
  type ProfileReportView,
  type ProfileReportStatus,
} from '../services/profileReport';
import Icon from './Icon.vue';
import ModalTransition from './ModalTransition.vue';

const props = defineProps<{
  epoch?: string;
  uid?: string;
  targetLabel: string;
  scope?: string;
  choice: ReportChoice | null;
  choices: readonly (readonly [ReportChoice, string])[];
  duplicateMessage: string;
  dispatch: (
    epoch: string,
    uid: string,
    command: ReportCommand,
  ) => Promise<AccountData<ProfileReportView>>;
  returnFocus: () => HTMLElement | null;
}>();
const headingId = useId();
const limitId = useId();
const emit = defineEmits<{ close: []; expired: [] }>();
const dialog = ref<HTMLDialogElement>();
const input = ref<HTMLTextAreaElement>();
const cancel = ref<HTMLButtonElement>();
const closeButton = ref<HTMLButtonElement>();
const text = ref('');
const status = ref<ProfileReportStatus>();
const checking = ref(false);
const busy = ref(false);
const issue = ref('');
let request = 0;
let alive = true;
const label = computed(() => props.choices.find(([key]) => key === props.choice)?.[1] ?? '');
const bytes = computed(() =>
  text.value.length > maxReportDescriptionBytes
    ? maxReportDescriptionBytes + 1
    : new TextEncoder().encode(text.value).length,
);
const tooLarge = computed(() => bytes.value > maxReportDescriptionBytes);
const maySubmit = computed(
  () =>
    !busy.value &&
    !checking.value &&
    (status.value === 'ready' || status.value === 'not_sent') &&
    (props.choice !== 'other' || (!!text.value.trim() && !tooLarge.value)),
);
const message = computed<Record<Exclude<ProfileReportStatus, 'ready'>, string>>(() => ({
  accepted: '举报成功',
  duplicate: props.duplicateMessage,
  official_account: '官方账号不能举报',
  uncertain: '未能确认举报结果。请勿重复提交。',
  not_sent: '举报未发送，可以重新确认后提交。',
  target_unavailable: '当前无法举报该对象。',
}));

function handleFailure(error: unknown) {
  const detail = failure(error);
  if (detail.code === 'session_expired' || detail.code === 'profile_changed') emit('expired');
}

async function inspect() {
  const { epoch, uid, choice, scope, dispatch } = props;
  if (!epoch || !uid || !choice || checking.value || busy.value) return;
  const ticket = ++request;
  const current = () =>
    alive &&
    ticket === request &&
    props.epoch === epoch &&
    props.uid === uid &&
    props.choice === choice &&
    props.scope === scope;
  checking.value = true;
  issue.value = '';
  try {
    const reply = await dispatch(epoch, uid, { kind: 'inspect' });
    if (current() && reply.epoch === epoch) status.value = reply.value.status;
  } catch (error) {
    if (!current()) return;
    issue.value = '暂无法核对本机举报回执。';
    handleFailure(error);
  } finally {
    if (current()) {
      checking.value = false;
      await nextTick();
      if (current() && status.value && !['ready', 'not_sent'].includes(status.value))
        closeButton.value?.focus({ preventScroll: true });
    }
  }
}

async function submit() {
  const { epoch, uid, choice, scope, dispatch } = props;
  if (!epoch || !uid || !choice || !maySubmit.value) return;
  const ticket = ++request;
  const current = () =>
    alive &&
    ticket === request &&
    props.epoch === epoch &&
    props.uid === uid &&
    props.choice === choice &&
    props.scope === scope;
  const reason: ReportReason =
    choice === 'other' ? { kind: 'other', description: text.value } : { kind: choice };
  busy.value = true;
  issue.value = '';
  let submitted = false;
  try {
    const prepared = await dispatch(epoch, uid, { kind: 'prepare', reason });
    const receipt = prepared.value.receipt;
    if (!current() || prepared.epoch !== epoch) {
      if (receipt) void dispatch(epoch, uid, { kind: 'cancel', receipt }).catch(() => {});
      return;
    }
    if (!receipt) {
      status.value = prepared.value.status;
      text.value = '';
      return;
    }
    submitted = true;
    const result = await dispatch(epoch, uid, { kind: 'submit', receipt });
    if (!current() || result.epoch !== epoch) return;
    status.value = result.value.status;
    if (status.value !== 'not_sent') text.value = '';
  } catch (error) {
    if (!current()) return;
    if (submitted) {
      status.value = 'uncertain';
      text.value = '';
    } else issue.value = '无法准备举报，请检查输入或稍后重试。';
    handleFailure(error);
  } finally {
    if (current()) {
      busy.value = false;
      await nextTick();
      if (current() && status.value !== 'ready' && status.value !== 'not_sent')
        closeButton.value?.focus({ preventScroll: true });
    }
  }
}

function close() {
  if (!busy.value) emit('close');
}
watch(
  () => [props.epoch, props.uid, props.choice, props.scope],
  async () => {
    request++;
    text.value = issue.value = '';
    status.value = undefined;
    checking.value = busy.value = false;
    const choice = props.choice;
    if (!choice || !props.epoch || !props.uid) return;
    await nextTick();
    if (choice !== props.choice || !dialog.value?.isConnected) return;
    if (!dialog.value.open) dialog.value.showModal();
    (choice === 'other' ? input.value : cancel.value)?.focus({ preventScroll: true });
    void inspect();
  },
  { flush: 'post', immediate: true },
);
onBeforeUnmount(() => {
  alive = false;
  request++;
});
</script>

<template>
  <Teleport to="body">
    <ModalTransition :return-focus="returnFocus" :fallback-focus="returnFocus">
      <dialog
        v-if="choice"
        ref="dialog"
        class="profile-report-dialog"
        :aria-labelledby="headingId"
        :aria-busy="busy || checking"
        data-moderation-report-dialog
        :data-profile-report-dialog="scope ? undefined : ''"
        @cancel.prevent="close"
      >
        <header>
          <h2 :id="headingId">{{ choice === 'other' ? '举报原因' : '确认举报' }}</h2>
          <button
            ref="closeButton"
            class="icon-button"
            type="button"
            aria-label="关闭举报"
            :disabled="busy"
            @click="close"
          >
            <Icon name="close" />
          </button>
        </header>
        <p class="muted profile-report-target">{{ targetLabel }}</p>
        <template v-if="!status || status === 'ready' || status === 'not_sent'">
          <label v-if="choice === 'other'" class="profile-report-description">
            <span class="sr-only">举报原因</span>
            <textarea
              ref="input"
              v-model="text"
              rows="4"
              placeholder="举报原因："
              spellcheck="false"
              autocomplete="off"
              :aria-describedby="limitId"
              :aria-invalid="tooLarge || undefined"
              :disabled="busy"
            />
            <small :id="limitId" :role="tooLarge ? 'alert' : undefined">
              {{ tooLarge ? '描述超过 4 KiB，请缩短后再提交。' : '最多 4 KiB。' }}
            </small>
          </label>
          <p v-else class="profile-report-preset">{{ label }}</p>
        </template>
        <p
          v-if="status && status !== 'ready'"
          role="status"
          data-moderation-report-outcome
          :data-profile-report-outcome="scope ? undefined : ''"
        >
          {{ message[status] }}
        </p>
        <p v-if="checking" class="muted" role="status">正在核对本机回执…</p>
        <p v-if="issue" role="alert">{{ issue }}</p>
        <footer>
          <button
            v-if="status === 'uncertain' || (!status && issue)"
            class="text-button"
            type="button"
            :disabled="busy || checking"
            @click="inspect"
          >
            核对本机回执
          </button>
          <button ref="cancel" class="secondary" type="button" :disabled="busy" @click="close">
            {{ status && !['ready', 'not_sent'].includes(status) ? '关闭' : '取消' }}
          </button>
          <button
            v-if="!status || status === 'ready' || status === 'not_sent'"
            class="primary"
            type="button"
            :disabled="!maySubmit"
            @click="submit"
          >
            {{ busy ? '正在提交…' : '确认' }}
          </button>
        </footer>
      </dialog>
    </ModalTransition>
  </Teleport>
</template>

<style scoped>
.profile-report-dialog {
  width: min(410px, calc(100vw - 32px));
  max-height: calc(100dvh - 32px);
  overflow: auto;
  padding: 20px;
  border: 1px solid var(--line);
  border-radius: 12px;
  color: var(--text);
  background: var(--surface);
  box-shadow: 0 14px 48px #0004;
}
.profile-report-dialog::backdrop {
  background: transparent;
}
.profile-report-dialog header,
.profile-report-dialog footer {
  display: flex;
  align-items: center;
  gap: 10px;
}
.profile-report-dialog header {
  justify-content: space-between;
}
.profile-report-dialog h2 {
  margin: 0;
  font-size: 18px;
}
.profile-report-target {
  margin: 2px 0 12px;
  overflow-wrap: anywhere;
}
.profile-report-description {
  display: grid;
  gap: 6px;
}
.profile-report-description textarea {
  width: 100%;
  box-sizing: border-box;
  resize: vertical;
  padding: 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  color: inherit;
  background: var(--surface-alt);
  font: inherit;
}
.profile-report-description small {
  color: var(--muted);
}
.profile-report-description small[role='alert'] {
  color: var(--danger);
}
.profile-report-preset {
  padding: 10px;
  border-radius: 8px;
  background: var(--surface-alt);
}
.profile-report-dialog footer {
  justify-content: end;
  margin-top: 18px;
}
</style>
