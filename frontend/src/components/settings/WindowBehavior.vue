<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue';
import { failure, type AccountView } from '../../services/contracts';
import { useCore } from '../../services/core';
import { useWindowPreferences } from '../../composables/useWindowPreferences';
import { useWindowLifecycle } from '../../composables/useWindowLifecycle';
import { useControlFocus } from '../../composables/useControlFocus';
import type { AutostartStatus, WindowSettings } from '../../services/window';
const props = defineProps<{ account: AccountView }>();
const core = useCore();
const autostart = ref<AutostartStatus>();
const autostartBusy = ref(false);
const autostartError = ref('');
let disposed = false;

async function readAutostart() {
  if (autostartBusy.value || disposed) return;
  autostartBusy.value = true;
  autostartError.value = '';
  try {
    const status = await core.autostartStatus();
    if (!disposed) autostart.value = status;
  } catch (error) {
    if (!disposed) {
      autostart.value = 'unavailable';
      autostartError.value = failure(error).message;
    }
  } finally {
    if (!disposed) autostartBusy.value = false;
  }
}

async function toggleAutostart() {
  const observed = autostart.value;
  if (autostartBusy.value || disposed || (observed !== 'enabled' && observed !== 'disabled'))
    return false;
  autostartBusy.value = true;
  autostartError.value = '';
  try {
    const status = await core.autostartChange(observed === 'disabled', observed);
    if (disposed) return false;
    autostart.value = status;
    return true;
  } catch (error) {
    if (!disposed) {
      autostartError.value = failure(error).message;
      // The OS may have completed the operation before the reply was lost.
      // Observe actual state; never silently issue the write again.
      try {
        const status = await core.autostartStatus();
        if (!disposed) autostart.value = status;
      } catch {
        if (!disposed) autostart.value = 'unavailable';
      }
    }
    return false;
  } finally {
    if (!disposed) autostartBusy.value = false;
  }
}

async function refreshSettings() {
  await Promise.all([model.read(), readAutostart()]);
}
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
onMounted(() => {
  void native.connect();
  void readAutostart();
});
onUnmounted(() => {
  disposed = true;
  model.dispose();
  native.dispose();
});
</script>
<template>
  <section
    class="settings-section window-behavior"
    aria-labelledby="settings-window"
    :aria-busy="model.loading.value || model.busy.value || autostartBusy"
  >
    <h2 id="settings-window">窗口</h2>
    <div class="settings-section-content">
      <div class="settings-read-status">
        <span>关闭主窗口时</span
        ><button
          ref="refresh"
          class="text-button"
          aria-label="重新读取窗口设置"
          :disabled="model.busy.value || model.loading.value || autostartBusy"
          @click="focus.run($event, refreshSettings)"
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
      <label class="settings-checkbox startup-registration">
        <input
          type="checkbox"
          name="settings-autostart"
          :checked="autostart === 'enabled'"
          :disabled="autostartBusy || (autostart !== 'enabled' && autostart !== 'disabled')"
          @click.prevent="focus.run($event, toggleAutostart)"
        /><span>开机自动运行</span>
      </label>
      <p v-if="autostartBusy" class="settings-note" role="status">正在核对系统启动项…</p>
      <p v-else-if="autostart === 'external'" class="settings-note" role="status">
        同名启动项由系统或其他程序管理，Cumulus 未修改。
      </p>
      <p v-else-if="autostart === 'unavailable'" class="settings-note" role="status">
        当前无法读取或管理系统启动项。
      </p>
      <p v-if="autostartError" class="catalog-error" role="alert">{{ autostartError }}</p>
      <fieldset
        v-if="model.view.value && !model.view.value.damaged"
        class="settings-options"
        :disabled="!model.canChange.value || autostart !== 'enabled' || autostartBusy"
      >
        <legend>自动启动时</legend>
        <div class="settings-choices">
          <label>
            <input
              type="radio"
              name="settings-startup-window-mode"
              value="minisize"
              :checked="
                autostart === 'enabled' && model.view.value.settings.startupMode === 'minisize'
              "
              @click.prevent="change('startupMode', 'minisize', $event)"
            /><span>最小化显示</span>
          </label>
          <label>
            <input
              type="radio"
              name="settings-startup-window-mode"
              value="front"
              :checked="
                autostart === 'enabled' && model.view.value.settings.startupMode === 'front'
              "
              @click.prevent="change('startupMode', 'front', $event)"
            /><span>前台显示</span>
          </label>
        </div>
      </fieldset>
      <p class="settings-note">
        登录系统时由系统安排启动；手动打开会正常显示。托盘不可用时会显示主窗口。
      </p>
      <p class="settings-note" role="status">
        {{ native.view.value?.trayAvailable ? '系统托盘可用' : '系统托盘暂不可用' }}
      </p>
      <p v-if="model.loading.value || model.busy.value" class="settings-note" role="status">
        {{ model.loading.value ? '正在读取…' : '正在保存…' }}
      </p>
      <p v-else-if="model.saved.value" class="settings-note" role="status">已保存。</p>
      <div v-if="model.view.value?.damaged" class="settings-recovery" role="alert">
        <p>本机窗口设置已损坏，暂不自动隐藏窗口。</p>
        <button
          class="secondary"
          :disabled="model.busy.value || model.loading.value"
          @click="focus.run($event, model.repair)"
        >
          恢复默认窗口设置
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
.startup-registration {
  margin-top: 12px;
}
</style>
