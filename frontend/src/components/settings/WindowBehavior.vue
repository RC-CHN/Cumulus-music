<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import { useWindowPreferences } from '../../composables/useWindowPreferences';
import { useWindowLifecycle } from '../../composables/useWindowLifecycle';
import { useControlFocus } from '../../composables/useControlFocus';
import type { WindowSettings } from '../../services/window';
const props = defineProps<{ account: AccountView }>();
const model = useWindowPreferences(),
  native = useWindowLifecycle();
const refresh = ref<HTMLButtonElement>();
const focus = useControlFocus(
  () => props.account.epoch,
  () => refresh.value,
);
function change<K extends keyof WindowSettings>(key: K, value: WindowSettings[K], event: Event) {
  return focus.run(event, () => model.change(key, value));
}
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
</script>
<template>
  <section
    class="settings-section window-behavior"
    aria-labelledby="settings-window"
    :aria-busy="model.loading.value || model.busy.value"
  >
    <h2 id="settings-window">窗口</h2>
    <div class="settings-section-content">
      <div class="settings-read-status">
        <span>关闭主窗口时</span
        ><button
          ref="refresh"
          class="text-button"
          aria-label="重新读取关闭设置"
          :disabled="model.busy.value || model.loading.value"
          @click="focus.run($event, model.read)"
        >
          刷新
        </button>
      </div>
      <fieldset
        v-if="model.view.value && !model.view.value.damaged"
        class="settings-options"
        :disabled="!model.canChange.value"
      >
        <legend class="sr-only">关闭方式</legend>
        <div class="settings-choices">
          <label
            ><input
              type="radio"
              name="settings-close-behavior"
              value="tray"
              :checked="model.view.value.settings.closeBehavior === 'tray'"
              @click.prevent="change('closeBehavior', 'tray', $event)"
            /><span>留在系统托盘</span></label
          >
          <label
            ><input
              type="radio"
              name="settings-close-behavior"
              value="exit"
              :checked="model.view.value.settings.closeBehavior === 'exit'"
              @click.prevent="change('closeBehavior', 'exit', $event)"
            /><span>退出 Cumulus</span></label
          >
          <label
            ><input
              type="checkbox"
              name="settings-close-remember"
              :checked="model.view.value.settings.rememberChoice"
              @click.prevent="
                change('rememberChoice', !model.view.value.settings.rememberChoice, $event)
              "
            /><span>不再提醒</span></label
          >
        </div>
      </fieldset>
      <p class="settings-note">保存在这台设备，立即生效。托盘不可用时会保留窗口并提示选择。</p>
      <p class="settings-note" role="status">
        {{ native.view.value?.trayAvailable ? '系统托盘可用' : '系统托盘暂不可用' }}
      </p>
      <p v-if="model.loading.value || model.busy.value" class="settings-note" role="status">
        {{ model.loading.value ? '正在读取…' : '正在保存…' }}
      </p>
      <p v-else-if="model.saved.value" class="settings-note" role="status">已保存。</p>
      <div v-if="model.view.value?.damaged" class="settings-recovery" role="alert">
        <p>本机关闭设置已损坏，暂不自动隐藏窗口。</p>
        <button
          class="secondary"
          :disabled="model.busy.value || model.loading.value"
          @click="focus.run($event, model.repair)"
        >
          恢复默认关闭设置
        </button>
      </div>
      <p v-if="model.error.value" class="catalog-error" role="alert">{{ model.error.value }}</p>
    </div>
  </section>
</template>
<style scoped>
.settings-choices {
  flex-direction: column;
  align-items: flex-start;
}
</style>
