<script setup lang="ts">
import { onUnmounted, ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import { useQueueClickPreferences } from '../../composables/useQueueClickPreferences';
import { useControlFocus } from '../../composables/useControlFocus';

const props = defineProps<{ account: AccountView }>();
const model = useQueueClickPreferences();
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
onUnmounted(model.dispose);
</script>

<template>
  <section
    class="settings-section queue-click-preferences"
    aria-labelledby="settings-queue-click"
    :aria-busy="model.loading.value || model.busy.value"
  >
    <h2 id="settings-queue-click">播放列表</h2>
    <div class="settings-section-content">
      <div class="settings-read-status">
        <span>双击歌曲时</span>
        <button
          ref="refresh"
          class="text-button"
          aria-label="重新读取播放列表设置"
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
        <legend>歌单、专辑及歌手歌曲列表</legend>
        <div class="settings-choices">
          <label>
            <input
              type="radio"
              name="queue-double-click"
              data-queue-click-choice="replace"
              :checked="model.view.value.replaceQueue"
              @click.prevent="focus.run($event, () => model.change(true))"
            />
            <span>用歌曲所在的列表替换播放列表</span>
          </label>
          <label>
            <input
              type="radio"
              name="queue-double-click"
              data-queue-click-choice="add"
              :checked="!model.view.value.replaceQueue"
              @click.prevent="focus.run($event, () => model.change(false))"
            />
            <span>仅将该歌曲加入播放列表并播放</span>
          </label>
        </div>
      </fieldset>
      <p class="settings-note">保存在这台设备。此设置只影响上述列表中的歌曲双击操作。</p>
      <p v-if="model.loading.value" class="settings-note" role="status">正在读取播放列表设置…</p>
      <p v-else-if="model.busy.value" class="settings-note" role="status">正在保存…</p>
      <p v-else-if="model.saved.value" class="settings-note" role="status">已保存</p>
      <div v-if="model.view.value?.damaged" class="settings-recovery" role="alert">
        <p>本机播放列表设置已损坏。恢复默认值后，双击歌曲会替换播放列表。</p>
        <button
          class="secondary"
          :disabled="model.busy.value || model.loading.value"
          @click="focus.run($event, model.repair)"
        >
          恢复默认播放列表设置
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
