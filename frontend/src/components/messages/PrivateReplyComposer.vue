<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useCore } from '../../services/core';
import type { MessageReplyView, MessageRow } from '../../services/messages';
import AccountBindingQrDialog from '../account/AccountBindingQrDialog.vue';

const props = defineProps<{
  epoch: string;
  scope: object;
  receipt: string;
  draft: string;
  uncertainAttempt?: string;
  blocked: boolean;
}>();
const emit = defineEmits<{
  draft: [scope: object, value: string];
  receipt: [scope: object, value: string];
  uncertainAttempt: [scope: object, value: string | undefined];
  delivered: [scope: object, row: MessageRow];
  busy: [scope: object, value: boolean];
  expired: [scope: object];
}>();
const core = useCore();
const scope = props.scope;
const editor = ref<HTMLTextAreaElement>();
const status = ref<MessageReplyView>();
const reading = ref(false);
const sending = ref(false);
const message = ref('');
const bindingQr = ref(false);
const armed = ref(props.receipt);
let visit = 0;
const locked = computed(() => status.value?.status === 'unknown' || !!props.uncertainAttempt);
const valid = computed(() => props.draft.length <= 1000 && /\S/.test(props.draft));
const editable = computed(() => !!status.value && !sending.value && !locked.value);
const canSend = computed(() => editable.value && valid.value && !props.blocked && !reading.value);
const isUnconfirmed = (value: MessageReplyView): boolean =>
  !!props.uncertainAttempt &&
  (!('operation' in value) || value.operation !== props.uncertainAttempt);

function useReceipt(value: MessageReplyView) {
  armed.value = value.receipt;
  if (value.receipt !== props.receipt) emit('receipt', scope, value.receipt);
}
function accept(value: MessageReplyView, acted?: string) {
  useReceipt(value);
  if (!acted && isUnconfirmed(value) && value.status !== 'unknown') {
    message.value = '本机还没有这次回复的回执，无法确认是否已发送。';
    return;
  }
  status.value = value;
  if (
    value.status !== 'unknown' &&
    (acted || ('operation' in value && value.operation === props.uncertainAttempt))
  )
    emit('uncertainAttempt', scope, undefined);
  switch (value.status) {
    case 'delivered':
      message.value = '已收到发送回执。';
      if (value.operation === acted || value.operation === props.uncertainAttempt)
        emit('draft', scope, '');
      if (value.row) emit('delivered', scope, value.row);
      break;
    case 'denied':
      message.value = value.intercepted
        ? '这条私信被服务拦截，内容已保留。'
        : '请求未发出，内容已保留。';
      break;
    case 'unknown':
      message.value = '无法确认私信是否已发送。客户端不会再次提交。';
      break;
    case 'phone_required':
      message.value = '发送私信需要绑定手机号。内容已保留。';
      if (acted) bindingQr.value = true;
      break;
    case 'preflight_unavailable':
      message.value = '暂时无法核对发送资格，本次未发送。';
      break;
    case 'ready':
      message.value = '';
  }
}
async function check(explicit = false) {
  if (reading.value || sending.value) return;
  const epoch = props.epoch;
  const step = ++visit;
  const live = () => step === visit && props.epoch === epoch;
  reading.value = true;
  try {
    const reply = await core.messageReply(epoch, { kind: 'status', receipt: armed.value });
    if (!live() || reply.epoch !== epoch) return;
    accept(reply.value);
    if (explicit && reply.value.status === 'unknown')
      message.value = '已核对本机回执，仍无法确认是否已发送。';
  } catch (error) {
    if (!live()) return;
    if ((error as { code?: string })?.code === 'session_expired') emit('expired', scope);
    message.value = '无法读取本机回复状态，请重试。';
  } finally {
    if (live()) reading.value = false;
  }
}
async function send() {
  if (!canSend.value) return;
  const epoch = props.epoch;
  const step = ++visit;
  const live = () => step === visit && props.epoch === epoch;
  const operation = armed.value;
  sending.value = true;
  // Keep the operation identity when Back closes this component before its
  // reply arrives. A new thread visit only clears that exact delivered draft.
  emit('uncertainAttempt', scope, operation);
  emit('busy', scope, true);
  message.value = '';
  try {
    const reply = await core.messageReply(epoch, {
      kind: 'send',
      receipt: operation,
      text: props.draft,
    });
    if (!live()) return;
    if (reply.epoch !== epoch) throw Error('stale reply acknowledgement');
    accept(reply.value, operation);
  } catch (error) {
    if (!live()) return;
    if ((error as { code?: string })?.code === 'session_expired') emit('expired', scope);
    emit('uncertainAttempt', scope, operation);
    message.value = '没有收到这次回复的结果，请先核对本机回执。';
  } finally {
    if (live()) {
      sending.value = false;
      emit('busy', scope, false);
      await nextTick();
      if (live() && !bindingQr.value && !locked.value) editor.value?.focus({ preventScroll: true });
    }
  }
}
function onEnter(event: KeyboardEvent) {
  if (event.key !== 'Enter' || event.shiftKey || event.isComposing || event.keyCode === 229) return;
  event.preventDefault();
  void send();
}
function onInput(event: Event) {
  if (editable.value && event.target instanceof HTMLTextAreaElement)
    emit('draft', scope, event.target.value);
}
async function closeQr() {
  bindingQr.value = false;
  await nextTick();
  editor.value?.focus({ preventScroll: true });
}
watch(
  () => props.receipt,
  (receipt) => {
    if (receipt === armed.value) return;
    armed.value = receipt;
    void check();
  },
);
watch(
  () => props.epoch,
  () => {
    visit++;
  },
  { flush: 'sync' },
);
onBeforeUnmount(() => {
  visit++;
  if (sending.value) emit('busy', scope, false);
});
void check();
</script>

<template>
  <form class="private-reply" aria-label="回复私信" @submit.prevent="send">
    <p v-if="message" data-message-reply-status role="status">{{ message }}</p>
    <button
      v-if="locked || (!status && !reading)"
      type="button"
      class="text-button"
      :disabled="reading"
      @click="check(true)"
    >
      {{ locked ? '核对本机回执' : '重试读取回复状态' }}
    </button>
    <button
      v-if="status?.status === 'phone_required'"
      type="button"
      class="text-button"
      @click="bindingQr = true"
    >
      打开绑定手机二维码
    </button>
    <textarea
      ref="editor"
      aria-label="私信正文"
      placeholder="说点什么吧"
      rows="2"
      :value="draft"
      :readonly="!editable"
      @input="onInput"
      @keydown="onEnter"
    />
    <footer>
      <span :class="{ 'private-reply-over': draft.length > 1000 }">{{ draft.length }} / 1000</span>
      <span v-if="draft.length > 1000" role="status">字数超出限制</span>
      <button type="submit" class="primary" :disabled="!canSend">
        {{ sending ? '正在发送…' : '发送' }}
      </button>
    </footer>
  </form>
  <div v-if="bindingQr" @keydown.esc.stop>
    <AccountBindingQrDialog :epoch="epoch" @close="closeQr" />
  </div>
</template>

<style scoped>
.private-reply {
  display: grid;
  gap: 5px;
  padding: 8px 14px;
  border-block-start: 1px solid var(--line);
}
.private-reply p {
  margin: 0;
  overflow-wrap: anywhere;
  color: var(--text-secondary);
}
.private-reply > .text-button {
  justify-self: start;
}
.private-reply textarea {
  box-sizing: border-box;
  width: 100%;
  min-height: 48px;
  padding: 5px 8px;
  border: 1px solid var(--line);
  border-radius: 5px;
  background: var(--surface);
  color: var(--text);
  resize: none;
}
.private-reply footer {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-secondary);
}
.private-reply footer .primary {
  margin-inline-start: auto;
  min-height: 32px;
  padding: 3px 16px;
}
.private-reply-over {
  color: var(--accent);
}
</style>
