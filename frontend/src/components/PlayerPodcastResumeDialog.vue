<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, useId } from 'vue';
import { useCore } from '../services/core';
import type {
  PlayerPodcastResumeAction,
  PlayerPodcastResumeTarget,
  PlayerPodcastResumeView,
} from '../services/playerPodcastResume';
import Icon from './Icon.vue';

const props = defineProps<{
  target: PlayerPodcastResumeTarget;
  initial: PlayerPodcastResumeView;
}>();
const emit = defineEmits<{ close: []; confirmed: [radioId: string, enabled: boolean] }>();
const client = useCore();
const title = useId();
const radio = useId();
const dialog = ref<HTMLDialogElement>();
const enabled = ref(props.initial.enabled);
const pending = ref(props.initial.pending);
const busy = ref(false);
const error = ref('');
let active = true;

onMounted(() => dialog.value?.showModal());
onBeforeUnmount(() => {
  active = false;
});

function close() {
  if (!busy.value) emit('close');
}

async function run(action: PlayerPodcastResumeAction) {
  const { epoch, playId, entryId } = props.target;
  const result = await client.playerPodcastResume(epoch, playId, entryId, action);
  if (!active || result.epoch !== epoch || !result.value) return null;
  pending.value = result.value.pending;
  return result.value;
}

async function submit() {
  if (busy.value) return;
  busy.value = true;
  error.value = '';
  try {
    const action = pending.value
      ? ({ kind: 'reconcile' } as const)
      : ({ kind: 'change', enabled: enabled.value } as const);
    const view = await run(action);
    if (!view || !view.pending) {
      if (view) emit('confirmed', view.radioId, view.enabled);
      if (active) emit('close');
    }
  } catch {
    if (!active) return;
    error.value = '未能确认断点续播设置';
    pending.value = true;
    try {
      // This passive view never sends a setting or repeats the previous POST.
      if (!(await run({ kind: 'view' })) && active) emit('close');
    } catch {
      // A hidden/stale view cannot authorize retry; the native guard remains final.
      if (active) pending.value = true;
    }
  } finally {
    if (active) busy.value = false;
  }
}
</script>

<template>
  <dialog
    ref="dialog"
    class="player-podcast-resume-dialog"
    data-player-podcast-resume
    :aria-labelledby="title"
    @cancel.prevent="close"
  >
    <header>
      <div>
        <h2 :id="title">断点续播</h2>
        <span>开启后仅对本播客的所有声音生效</span>
      </div>
      <button
        type="button"
        class="icon-button"
        aria-label="关闭断点续播设置"
        :disabled="busy"
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <form @submit.prevent="submit">
      <fieldset aria-label="断点续播" :disabled="busy || pending">
        <label>
          <input
            type="radio"
            :name="radio"
            :checked="enabled"
            :autofocus="enabled"
            @change="enabled = true"
          />
          <strong>开启</strong><span>从上一次停止的地方继续播放</span>
        </label>
        <label>
          <input
            type="radio"
            :name="radio"
            :checked="!enabled"
            :autofocus="!enabled"
            @change="enabled = false"
          />
          <strong>关闭</strong><span>每次从头开始播放</span>
        </label>
      </fieldset>
      <p v-if="pending || error" role="status">{{ error || '上次设置尚未确认' }}</p>
      <footer>
        <button class="primary" type="submit" :aria-disabled="busy">
          {{ pending ? '确认设置结果' : '确定' }}
        </button>
      </footer>
    </form>
  </dialog>
</template>

<style scoped>
.player-podcast-resume-dialog {
  box-sizing: border-box;
  width: min(442px, calc(100vw - 32px));
  max-height: calc(100dvh - 32px);
  margin: auto;
  padding: 20px 24px;
  border: 0;
  border-radius: 12px;
  color: var(--text);
  background: var(--surface);
  box-shadow: 0 24px 90px #24132833;
  overflow: auto;
}
.player-podcast-resume-dialog::backdrop {
  background: var(--overlay);
  backdrop-filter: blur(3px);
}
header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
}
h2 {
  margin: 0 0 5px;
  font-size: 18px;
}
header span,
label span,
p {
  color: var(--muted);
  font-size: 13px;
}
fieldset {
  display: grid;
  gap: 12px;
  border: 0;
  margin: 22px 0 16px;
  padding: 0;
}
label {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 26px;
  cursor: pointer;
}
label input {
  accent-color: var(--accent);
}
footer {
  display: flex;
  justify-content: flex-end;
}
footer button {
  min-width: 86px;
  min-height: 32px;
  padding: 4px 14px;
  border: 0;
  border-radius: 6px;
  color: var(--on-accent);
  background: var(--accent);
}
footer button[aria-disabled='true'] {
  opacity: 0.55;
}
</style>
