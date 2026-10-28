<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import type { OutputEngine } from '../../services/output';
import { useOutputSettings } from '../../composables/useOutputSettings';
import { useControlFocus } from '../../composables/useControlFocus';
const props = defineProps<{ account: AccountView }>();
const model = useOutputSettings();
const refresh = ref<HTMLButtonElement>();
const focus = useControlFocus(
  () => props.account.epoch,
  () => refresh.value,
);
const devices = computed(() => [
  { id: 'auto', description: '系统默认' },
  ...(model.view.value?.devices.filter((d) => d.id !== 'auto') ?? []),
]);
const missing = computed(
  () => model.view.value && !devices.value.some((d) => d.id === model.view.value?.selected.id),
);
const canRetry = computed(() =>
  model.view.value?.engines.some((e) => ['fallback', 'failed'].includes(e.phase)),
);
function status(e: OutputEngine) {
  if (e.phase === 'pending') return '等待播放器应用';
  if (e.phase === 'dormant') return '下次播放时应用';
  if (e.phase === 'unsupported') return '当前播放引擎不支持选择输出';
  if (e.phase === 'failed')
    return e.issue === 'enumeration'
      ? '暂时无法读取设备状态，请刷新重试'
      : '输出设置未能应用，请检查设备后重试';
  if (e.phase === 'fallback')
    return e.issue === 'missing_device'
      ? '所选设备不可用，已回退到系统默认'
      : '已回退到系统默认，可重试所选设备';
  return '播放器已应用输出设置';
}
function select(event: Event) {
  const target = event.target as HTMLSelectElement;
  const id = target.value;
  return focus.run(event, async () => {
    await model.select(id);
    if (target.isConnected) target.value = model.view.value?.selected.id ?? 'auto';
  });
}
watch(
  () => props.account.epoch,
  () => void model.connect(props.account),
  { immediate: true },
);
onUnmounted(model.dispose);
</script>
<template>
  <section
    class="settings-section audio-output"
    aria-labelledby="settings-audio-output"
    :aria-busy="model.loading.value || model.busy.value"
  >
    <h2 id="settings-audio-output">音频输出</h2>
    <div class="settings-section-content">
      <div class="settings-read-status">
        <span>本机输出设备</span
        ><button
          ref="refresh"
          class="text-button"
          aria-label="刷新音频输出设备"
          :disabled="model.loading.value || model.busy.value"
          @click="focus.run($event, () => model.read())"
        >
          刷新
        </button>
      </div>
      <div v-if="model.view.value && !model.view.value.damaged" class="output-selection">
        <label for="audio-output-device">输出到</label>
        <select
          id="audio-output-device"
          :value="model.view.value.selected.id"
          :disabled="!model.canChange.value"
          @change="select"
        >
          <option v-if="missing" :value="model.view.value.selected.id" disabled>
            {{ model.view.value.selected.description }}（暂不可用）
          </option>
          <option v-for="device in devices" :key="device.id" :value="device.id">
            {{ device.description }}
          </option>
        </select>
      </div>
      <p class="settings-note">
        音乐和 MV 共用此设置，保存在这台设备。设备不可用时尝试系统默认；重新连接后可手动重试。
      </p>
      <p v-if="model.loading.value" class="settings-note" role="status">正在读取输出设备…</p>
      <p v-else-if="model.busy.value" class="settings-note" role="status">正在保存输出设置…</p>
      <p v-else-if="model.saved.value" class="settings-note" role="status">输出偏好已保存。</p>
      <p v-if="model.view.value?.supported === false" class="settings-note" role="status">
        当前播放引擎不提供输出设备选择。
      </p>
      <div v-else-if="model.view.value" class="output-status" aria-live="polite">
        <p v-if="!model.view.value.engines.length" class="settings-note">
          下次播放时应用所选输出。
        </p>
        <p v-for="engine in model.view.value.engines" :key="engine.kind" class="settings-note">
          {{ engine.kind === 'audio' ? '音乐' : 'MV' }}：{{ status(engine) }}
        </p>
      </div>
      <button
        v-if="canRetry"
        class="secondary"
        :disabled="!model.canChange.value"
        @click="focus.run($event, model.retry)"
      >
        重试所选输出
      </button>
      <div v-if="model.view.value?.damaged" class="settings-recovery" role="alert">
        <p>输出偏好已损坏，暂用系统默认。恢复时会保留原记录供诊断。</p>
        <button
          class="secondary"
          :disabled="model.loading.value || model.busy.value"
          @click="focus.run($event, model.repair)"
        >
          恢复默认输出设置
        </button>
      </div>
      <p v-if="model.error.value" class="catalog-error" role="alert">{{ model.error.value }}</p>
    </div>
  </section>
</template>
<style scoped>
.output-selection {
  display: flex;
  align-items: center;
  gap: 16px;
  min-width: 0;
  margin: 16px 0;
}
.output-selection label {
  flex: 0 0 auto;
}
.output-selection select {
  appearance: none;
  flex: 1 1 0;
  width: 0;
  max-width: 420px;
  min-width: 0;
  padding: 8px 34px 8px 12px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: var(--surface);
  background-image:
    linear-gradient(45deg, transparent 50%, var(--muted) 50%),
    linear-gradient(135deg, var(--muted) 50%, transparent 50%);
  background-position:
    calc(100% - 18px) 50%,
    calc(100% - 13px) 50%;
  background-size: 5px 5px;
  background-repeat: no-repeat;
  color: var(--text);
  font: inherit;
  text-overflow: ellipsis;
  overflow: hidden;
  white-space: nowrap;
}
.output-status p {
  margin: 6px 0;
}
@media (max-width: 680px) {
  .output-selection {
    align-items: stretch;
    flex-direction: column;
    gap: 8px;
  }
  .output-selection select {
    flex: 0 0 auto;
    width: 100%;
  }
}
</style>
