<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, useId } from 'vue';
import {
  audiobookSkipDraft,
  audiobookSkipSaved,
  audiobookSkipSetting,
  chooseAudiobookSkip,
  MAX_SKIP_SECONDS,
  moveAudiobookSkip,
  type AudiobookSkipContext,
  type AudiobookSkipView,
  type SkipDraft,
} from '../services/audiobookSkip';
import { useCore } from '../services/core';
import Icon from './Icon.vue';

const props = defineProps<{ epoch: string; context: AudiobookSkipContext }>();
const emit = defineEmits<{ close: [] }>();
const client = useCore();
const titleId = useId();
const radioName = useId();
const dialog = ref<HTMLDialogElement>();
const view = shallowRef<AudiobookSkipView>();
const draft = shallowRef<SkipDraft>();
const pending = ref(false);
const saving = ref(false);
const needsReadback = ref(false);
const error = ref('');
const setting = computed(() => (draft.value ? audiobookSkipSetting(draft.value) : undefined));
let alive = true;

onMounted(() => {
  dialog.value?.showModal();
  void load();
});
onBeforeUnmount(() => {
  alive = false;
});

async function load() {
  if (pending.value) return;
  pending.value = true;
  error.value = '';
  try {
    const result = await client.audiobookSkipSettings(props.epoch, props.context, { kind: 'open' });
    if (!alive || result.epoch !== props.epoch) return;
    view.value = result.value;
    draft.value = audiobookSkipDraft(result.value.setting);
    needsReadback.value = false;
    void readSmart(result.value.editId);
  } catch {
    if (alive) error.value = '无法读取跳过设置';
  } finally {
    if (alive) pending.value = false;
  }
}
async function readSmart(editId: string) {
  try {
    const result = await client.audiobookSkipSettings(props.epoch, props.context, {
      kind: 'smart',
      editId,
    });
    if (alive && result.epoch === props.epoch && view.value?.editId === editId) {
      // The independent availability result must not replace an edited draft.
      view.value = result.value;
    }
  } catch {
    // Manual and off remain local and usable when the feature read is absent.
  }
}
async function save() {
  if (!view.value || !setting.value || pending.value || needsReadback.value) return;
  pending.value = true;
  saving.value = true;
  error.value = '';
  try {
    const result = await client.audiobookSkipSettings(props.epoch, props.context, {
      kind: 'save',
      editId: view.value.editId,
      setting: setting.value,
    });
    if (result.epoch !== props.epoch) return;
    audiobookSkipSaved.value = result;
    if (alive) emit('close');
  } catch {
    if (alive) {
      needsReadback.value = true;
      error.value = '未能确认保存结果，请重新读取本机设置';
    }
  } finally {
    if (alive) {
      pending.value = false;
      saving.value = false;
    }
  }
}
function choose(kind: SkipDraft['kind']) {
  if (draft.value) draft.value = chooseAudiobookSkip(draft.value, kind);
}
function move(rail: 'start' | 'endRail', event: Event) {
  if (draft.value)
    draft.value = moveAudiobookSkip(
      draft.value,
      rail,
      (event.currentTarget as HTMLInputElement).valueAsNumber,
    );
}
</script>

<template>
  <dialog
    ref="dialog"
    class="audiobook-skip-dialog"
    :aria-labelledby="titleId"
    @cancel.prevent="emit('close')"
  >
    <header>
      <div class="audiobook-skip-heading">
        <h2 :id="titleId">跳过片头片尾</h2>
        <span>开启后仅对本播单的所有声音有效</span>
      </div>
      <button type="button" class="icon-button" aria-label="关闭跳过设置" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
    <form v-if="draft" class="audiobook-skip-content" @submit.prevent="save">
      <fieldset :disabled="pending" aria-label="跳过方式">
        <label>
          <input
            type="radio"
            :name="radioName"
            :checked="draft.kind === 'close'"
            @change="choose('close')"
          />
          <strong>不开启</strong><span>不跳过片头片尾</span>
        </label>
        <label v-if="view?.smartAvailable">
          <input
            type="radio"
            :name="radioName"
            :checked="draft.kind === 'smart'"
            @change="choose('smart')"
          />
          <strong>智能开启</strong><span>将根据不同声音智能跳过片头片尾</span>
        </label>
        <label>
          <input
            type="radio"
            :name="radioName"
            :checked="draft.kind === 'custom'"
            @change="choose('custom')"
          />
          <strong>手动设置</strong><span>每个声音都将以同样的设置跳过</span>
        </label>
      </fieldset>
      <div v-if="draft.kind === 'custom'" class="audiobook-skip-intervals">
        <div class="audiobook-skip-rails">
          <input
            type="range"
            min="0"
            :max="MAX_SKIP_SECONDS"
            step="1"
            aria-label="跳过片头"
            :aria-valuetext="`${draft.start} 秒`"
            :style="{ '--skip-cut': `${(draft.start / MAX_SKIP_SECONDS) * 100}%` }"
            :value="draft.start"
            :disabled="pending"
            @input="move('start', $event)"
            @pointerup="move('start', $event)"
          />
          <span aria-hidden="true" />
          <input
            class="outro"
            type="range"
            min="0"
            :max="MAX_SKIP_SECONDS"
            step="1"
            aria-label="跳过片尾"
            :aria-valuetext="`${MAX_SKIP_SECONDS - draft.endRail} 秒`"
            :style="{ '--skip-cut': `${(draft.endRail / MAX_SKIP_SECONDS) * 100}%` }"
            :value="draft.endRail"
            :disabled="pending"
            @input="move('endRail', $event)"
            @pointerup="move('endRail', $event)"
          />
        </div>
        <div class="audiobook-skip-labels">
          <span>跳过片头 {{ draft.start }} 秒</span>
          <span>跳过片尾 {{ MAX_SKIP_SECONDS - draft.endRail }} 秒</span>
        </div>
      </div>
      <div v-if="error" role="alert">
        {{ error }}
        <button
          v-if="needsReadback"
          type="button"
          class="text-button"
          :disabled="pending"
          @click="load"
        >
          重新读取
        </button>
      </div>
      <button
        type="submit"
        class="primary audiobook-skip-confirm"
        :disabled="pending || needsReadback || !setting"
      >
        {{ saving ? '正在保存…' : '确定' }}
      </button>
    </form>
    <div v-else class="audiobook-skip-content">
      <p v-if="pending" role="status">正在读取设置…</p>
      <div v-else-if="error" role="alert">
        {{ error }}
        <button type="button" class="text-button" @click="load">重试</button>
      </div>
    </div>
  </dialog>
</template>

<style scoped>
.audiobook-skip-dialog {
  box-sizing: border-box;
  width: min(442px, calc(100vw - 32px));
  max-height: calc(100dvh - 32px);
  margin: auto;
  padding: 22px 24px 24px;
  border: 0;
  border-radius: 12px;
  color: var(--text);
  background: var(--surface);
  box-shadow: 0 24px 90px #24132833;
  overflow: auto;
}
.audiobook-skip-dialog::backdrop {
  background: var(--overlay);
  backdrop-filter: blur(3px);
}
.audiobook-skip-dialog header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 20px;
}
.audiobook-skip-heading {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 6px 12px;
}
.audiobook-skip-heading h2 {
  font-size: 20px;
  margin: 0;
}
.audiobook-skip-heading span,
.audiobook-skip-content label span {
  color: var(--muted);
  font-size: 12px;
}
.audiobook-skip-content fieldset {
  margin: 0;
  padding: 0;
  border: 0;
  display: grid;
  gap: 18px;
}
.audiobook-skip-content label {
  display: flex;
  align-items: center;
  gap: 8px;
}
.audiobook-skip-content label strong {
  font-size: 14px;
  font-weight: 500;
  white-space: nowrap;
}
.audiobook-skip-content input[type='radio'] {
  accent-color: var(--accent);
  margin: 0;
}
.audiobook-skip-intervals {
  margin: 24px 8px 0;
}
.audiobook-skip-rails {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(18px, 47px) minmax(0, 1fr);
  gap: 4px;
  align-items: center;
}
.audiobook-skip-rails span {
  height: 6px;
  background: var(--accent);
  opacity: 0.22;
  border-radius: 9px;
}
.audiobook-skip-rails input {
  appearance: none;
  width: 100%;
  height: 6px;
  margin: 5px 0;
  border-radius: 9px;
  background: linear-gradient(to right, var(--line) var(--skip-cut), var(--accent) var(--skip-cut));
}
.audiobook-skip-rails .outro {
  background: linear-gradient(to right, var(--accent) var(--skip-cut), var(--line) var(--skip-cut));
}
.audiobook-skip-rails input::-webkit-slider-thumb {
  appearance: none;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--accent);
}
.audiobook-skip-labels {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  margin-top: 9px;
  color: var(--muted);
  font-size: 13px;
}
.audiobook-skip-confirm {
  display: block;
  width: min(240px, 100%);
  min-height: 40px;
  margin: 20px auto 0;
  border-radius: 80px;
}
</style>
