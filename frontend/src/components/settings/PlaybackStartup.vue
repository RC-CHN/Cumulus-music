<script setup lang="ts">
import { onUnmounted, ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import { useStartupPreferences } from '../../composables/useStartupPreferences';
import { useControlFocus } from '../../composables/useControlFocus';
import type { StartupSettings } from '../../services/startup';
const props = defineProps<{ account: AccountView }>();
const model = useStartupPreferences();
const refresh = ref<HTMLButtonElement>();
const controlFocus = useControlFocus(
  () => props.account.epoch,
  () => refresh.value,
);
function change(key: keyof StartupSettings, value: boolean, event: Event) {
  return controlFocus.run(event, () => model.change(key, value));
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
    class="settings-section playback-startup"
    aria-labelledby="settings-playback"
    :aria-busy="model.loading.value || model.busy.value"
  >
    <h2 id="settings-playback">播放</h2>
    <div class="settings-section-content">
      <div class="settings-read-status">
        <span>启动行为</span
        ><button
          ref="refresh"
          class="text-button"
          aria-label="重新读取播放设置"
          :disabled="model.busy.value || model.loading.value"
          @click="controlFocus.run($event, model.read)"
        >
          刷新
        </button>
      </div>
      <fieldset
        v-if="model.view.value && !model.view.value.damaged"
        class="settings-options"
        :disabled="!model.canChange.value"
      >
        <legend>启动时</legend>
        <div class="settings-choices">
          <label
            ><input
              type="checkbox"
              name="startup-auto-play"
              :checked="model.view.value?.settings.autoPlay ?? false"
              @click.prevent="change('autoPlay', !model.view.value?.settings.autoPlay, $event)"
            /><span>程序启动时自动播放</span></label
          >
          <label
            ><input
              type="checkbox"
              name="startup-remember-position"
              :checked="model.view.value?.settings.rememberPosition ?? false"
              @click.prevent="
                change('rememberPosition', !model.view.value?.settings.rememberPosition, $event)
              "
            /><span>程序启动时记住上一次播放进度</span></label
          >
        </div>
      </fieldset>
      <p class="settings-note">
        保存在这台设备，下次启动生效。切换账号时恢复为暂停；关闭进度记忆时从歌曲或片段开头播放。
      </p>
      <p v-if="model.loading.value" class="settings-note" role="status">正在读取播放设置…</p>
      <p v-else-if="model.busy.value" class="settings-note" role="status">正在保存…</p>
      <p v-else-if="model.saved.value" class="settings-note" role="status">
        已保存，下次启动生效。
      </p>
      <div v-if="model.view.value?.damaged" class="settings-recovery" role="alert">
        <p>本机播放设置已损坏，暂不自动播放。恢复默认值后，自动播放关闭、进度记忆开启。</p>
        <button
          class="secondary"
          :disabled="model.busy.value || model.loading.value"
          @click="controlFocus.run($event, model.repair)"
        >
          恢复默认播放设置
        </button>
      </div>
      <p v-if="model.error.value" role="alert" class="catalog-error">{{ model.error.value }}</p>
    </div>
  </section>
</template>
<style scoped>
.settings-choices {
  flex-direction: column;
  align-items: flex-start;
}
</style>
