<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { PlayerModel } from '../../composables/usePlayer';
import { useControlFocus } from '../../composables/useControlFocus';
import { failure } from '../../services/contracts';
const props = defineProps<{ model: PlayerModel; epoch: string }>();
const view = computed(() => props.model.view.value?.loudness);
const refresh = ref<HTMLButtonElement>();
const focus = useControlFocus(
  () => props.epoch,
  () => refresh.value,
);
const busy = ref(false);
const error = ref('');
let request = 0;
const disabled = computed(() => !props.model.ready.value || busy.value);
watch(
  () => props.epoch,
  () => {
    request++;
    busy.value = false;
    error.value = '';
  },
);
async function save(input: Parameters<PlayerModel['configureLoudness']>[0]) {
  if (disabled.value) return;
  const current = ++request;
  busy.value = true;
  error.value = '';
  try {
    await props.model.configureLoudness(input);
  } catch (e) {
    if (current === request) error.value = failure(e).message;
  } finally {
    if (current === request) busy.value = false;
  }
}
const labels = {
  off: '已关闭',
  no_track: '已开启，播放时根据歌曲的音量信息调整',
  no_gain: '当前音源没有可用的音量调整信息，保持原始音量',
  invalid_gain: '当前音源的音量信息异常，保持原始音量',
  pending: '设置已保存，等待当前音源加载完成',
  configured: '已配置，等待音频处理',
  processing: '正在平衡当前歌曲的音量',
  unavailable: '音量均衡组件暂不可用，保持原始音量',
  failed: '音量均衡未能应用，可重新尝试',
};
</script>
<template>
  <section
    class="settings-section loudness-settings"
    aria-labelledby="settings-loudness"
    :aria-busy="busy"
    :data-revision="view?.revision"
  >
    <h2 id="settings-loudness">音量均衡</h2>
    <div class="settings-section-content">
      <div class="settings-read-status">
        <span>平衡不同音频内容之间的音量大小</span>
        <button
          ref="refresh"
          class="text-button"
          :disabled="disabled"
          aria-label="重新应用音量均衡"
          @click="focus.run($event, () => save({ kind: 'retry_loudness' }))"
        >
          重新应用
        </button>
      </div>
      <label v-if="view && !view.damaged" class="settings-checkbox">
        <input
          type="checkbox"
          name="loudness-enabled"
          :checked="view.enabled"
          :disabled="disabled || !view.revision"
          @click.prevent="
            focus.run($event, () =>
              save({
                kind: 'loudness',
                expected_revision: view!.revision!,
                enabled: !view!.enabled,
              }),
            )
          "
        />
        <span>启用音量均衡</span>
      </label>
      <p class="settings-note">保存在这台设备。没有音量调整信息的音源会保持原始音量。</p>
      <p v-if="view" class="settings-note loudness-status" role="status" :data-phase="view.status">
        {{ labels[view.status] }}
      </p>
      <div v-if="view?.damaged" class="settings-recovery" role="alert">
        <p>本机音量均衡设置已损坏，当前按关闭处理。恢复默认设置会保留损坏数据的备份。</p>
        <button
          class="secondary"
          :disabled="disabled"
          @click="focus.run($event, () => save({ kind: 'repair_loudness' }))"
        >
          恢复默认音量均衡设置
        </button>
      </div>
      <p v-if="error" class="catalog-error" role="alert">{{ error }}</p>
    </div>
  </section>
</template>
