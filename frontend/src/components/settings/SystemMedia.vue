<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import { systemMediaLabels } from '../../services/systemMedia';
import { useSystemMediaPreferences } from '../../composables/useSystemMediaPreferences';
import { useSystemMediaStatus } from '../../composables/useSystemMediaStatus';
import { useControlFocus } from '../../composables/useControlFocus';
const props = defineProps<{ account: AccountView }>();
const model = useSystemMediaPreferences(),
  native = useSystemMediaStatus();
const refresh = ref<HTMLButtonElement>();
const focus = useControlFocus(
  () => props.account.epoch,
  () => refresh.value,
);
watch(
  () => props.account.epoch,
  () => void model.connect(props.account),
  { immediate: true },
);
onMounted(native.connect);
onUnmounted(() => {
  model.dispose();
  native.dispose();
});
async function read() {
  await Promise.all([model.read(), native.read()]);
}
</script>
<template>
  <section
    class="settings-section system-media-settings"
    aria-labelledby="settings-system-media"
    :aria-busy="model.loading.value || model.busy.value"
  >
    <h2 id="settings-system-media">系统媒体控制</h2>
    <div class="settings-section-content">
      <div class="settings-read-status">
        <span>从系统媒体面板控制当前播放</span>
        <button
          ref="refresh"
          class="text-button"
          aria-label="重新读取系统媒体设置"
          :disabled="model.busy.value || model.loading.value"
          @click="focus.run($event, read)"
        >
          刷新
        </button>
      </div>
      <label v-if="model.view.value && !model.view.value.damaged" class="settings-checkbox">
        <input
          type="checkbox"
          name="system-media-enabled"
          :checked="model.view.value.enabled"
          :disabled="!model.canChange.value || !native.view.value?.installed"
          @click.prevent="focus.run($event, () => model.change(!model.view.value!.enabled))"
        />
        <span>启用系统媒体控制</span>
      </label>
      <p class="settings-note">保存在这台设备。关闭后，音乐仍会继续播放。</p>
      <p
        v-if="native.view.value"
        class="settings-note media-status"
        role="status"
        :data-phase="native.view.value.phase"
      >
        {{ systemMediaLabels[native.view.value.phase] }}
      </p>
      <p v-if="model.loading.value || model.busy.value" class="settings-note" role="status">
        {{ model.loading.value ? '正在读取…' : '正在保存…' }}
      </p>
      <p v-else-if="model.saved.value" class="settings-note" role="status">已保存。</p>
      <div v-if="model.view.value?.damaged" class="settings-recovery" role="alert">
        <p>本机设置已损坏。恢复默认设置后可重新启用系统媒体控制。</p>
        <button
          class="secondary"
          :disabled="model.busy.value || model.loading.value"
          @click="focus.run($event, model.repair)"
        >
          恢复默认系统媒体设置
        </button>
      </div>
      <p v-if="model.error.value || native.error.value" class="catalog-error" role="alert">
        {{ model.error.value || native.error.value }}
      </p>
    </div>
  </section>
</template>
