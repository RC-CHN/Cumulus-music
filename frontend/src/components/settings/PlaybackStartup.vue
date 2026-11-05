<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import { useStartupPreferences } from '../../composables/useStartupPreferences';
import { useControlFocus } from '../../composables/useControlFocus';
import { usePlaybackPower } from '../../composables/usePlaybackPower';
import { useSceneFmAutoplay } from '../../composables/useSceneFmAutoplay';
import { useCore } from '../../services/core';
import type { StartupSettings } from '../../services/startup';
const props = defineProps<{ account: AccountView }>();
const model = useStartupPreferences();
const power = usePlaybackPower();
const sceneFm = useSceneFmAutoplay(useCore());
const showSceneFm = computed(() => props.account.status === 'authenticated');
const startupVisible = computed(() => !!model.view.value && !model.view.value.damaged);
const busy = computed(
  () =>
    model.loading.value ||
    model.busy.value ||
    power.loading.value ||
    power.busy.value ||
    sceneFm.loading.value ||
    sceneFm.busy.value,
);
const refresh = ref<HTMLButtonElement>();
const controlFocus = useControlFocus(
  () => props.account.epoch,
  () => refresh.value,
);
function change(key: keyof StartupSettings, value: boolean, event: Event) {
  return controlFocus.run(event, () => model.change(key, value));
}
function read() {
  return Promise.all([model.read(), power.read(), sceneFm.read()]);
}
watch(
  () => [props.account.epoch, props.account.status] as const,
  ([epoch]) => {
    void model.connect(props.account);
    void power.connect(epoch);
    void sceneFm.connect(props.account);
  },
  { immediate: true },
);
onUnmounted(() => {
  model.dispose();
  power.dispose();
  sceneFm.dispose();
});
</script>
<template>
  <section
    class="settings-section playback-startup"
    aria-labelledby="settings-playback"
    :aria-busy="busy"
  >
    <h2 id="settings-playback" tabindex="-1">播放</h2>
    <div class="settings-section-content">
      <div class="settings-read-status">
        <span>{{ power.visible.value || showSceneFm ? '播放行为' : '启动行为' }}</span
        ><button
          ref="refresh"
          class="text-button"
          aria-label="重新读取播放设置"
          :disabled="busy"
          @click="controlFocus.run($event, read)"
        >
          刷新
        </button>
      </div>
      <fieldset
        v-if="startupVisible || power.visible.value || showSceneFm"
        class="settings-options"
      >
        <legend>{{ power.visible.value || showSceneFm ? '播放选项' : '启动时' }}</legend>
        <div class="settings-choices">
          <label v-if="startupVisible">
            <input
              type="checkbox"
              name="startup-auto-play"
              :checked="model.view.value?.settings.autoPlay ?? false"
              :disabled="!model.canChange.value"
              @click.prevent="change('autoPlay', !model.view.value?.settings.autoPlay, $event)"
            /><span>程序启动时自动播放</span>
          </label>
          <label v-if="power.visible.value">
            <input
              type="checkbox"
              name="playback-keep-awake"
              :checked="power.view.value?.enabled ?? false"
              :disabled="!power.canChange.value"
              :aria-describedby="power.error.value ? 'playback-power-error' : undefined"
              @click.prevent="
                controlFocus.run($event, () => power.change(!power.view.value?.enabled))
              "
            /><span>音乐播放时不自动休眠或熄屏</span>
          </label>
          <label v-if="showSceneFm">
            <input
              type="checkbox"
              name="scene-fm-autoplay"
              data-scene-fm-autoplay
              aria-label="首次进入播客页时自动播放"
              aria-describedby="scene-fm-autoplay-hint"
              :checked="sceneFm.enabled.value ?? false"
              :disabled="!sceneFm.canChange.value"
              @click.prevent="
                controlFocus.run($event, () => sceneFm.change(!sceneFm.enabled.value))
              "
            /><span>首次进入播客页时自动播放</span>
            <span id="scene-fm-autoplay-hint" class="muted">（不播歌时）</span>
          </label>
          <label v-if="startupVisible">
            <input
              type="checkbox"
              name="startup-remember-position"
              :checked="model.view.value?.settings.rememberPosition ?? false"
              :disabled="!model.canChange.value"
              @click.prevent="
                change('rememberPosition', !model.view.value?.settings.rememberPosition, $event)
              "
            /><span>程序启动时记住上一次播放进度</span>
          </label>
        </div>
      </fieldset>
      <template v-if="showSceneFm">
        <p class="settings-note">
          播客选项保存在当前账号，下次进入含随心听 FM 的播客页时生效；保存不会立即播放。
        </p>
        <p v-if="sceneFm.loading.value" class="settings-note" role="status">
          正在读取播客自动播放设置…
        </p>
        <p v-else-if="sceneFm.busy.value" class="settings-note" role="status">
          正在保存播客自动播放设置…
        </p>
        <p v-else-if="sceneFm.saved.value" class="settings-note" role="status">
          播客自动播放设置已保存。
        </p>
        <p
          v-if="sceneFm.error.value"
          class="catalog-error"
          data-scene-fm-autoplay-error
          role="alert"
        >
          {{ sceneFm.error.value }}
        </p>
      </template>
      <template v-if="power.visible.value">
        <p v-if="power.busy.value" class="settings-note" data-playback-power-status role="status">
          正在保存…
        </p>
        <p
          v-else-if="power.saved.value"
          class="settings-note"
          data-playback-power-status
          role="status"
        >
          设置已更新
        </p>
        <p
          v-if="power.error.value"
          id="playback-power-error"
          class="catalog-error"
          data-playback-power-error
          role="alert"
        >
          {{ power.error.value }}
        </p>
      </template>
      <p class="settings-note">
        {{
          power.visible.value || showSceneFm
            ? '启动选项保存在这台设备，下次启动生效。'
            : '保存在这台设备，下次启动生效。'
        }}
        切换账号时恢复为暂停；关闭进度记忆时从歌曲或片段开头播放。
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
#settings-playback:focus-visible {
  outline: none;
  text-decoration: underline 2px var(--accent);
  text-underline-offset: 0.15em;
}
.settings-choices {
  flex-direction: column;
  align-items: flex-start;
}
</style>
