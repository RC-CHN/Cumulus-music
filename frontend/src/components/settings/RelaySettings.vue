<script setup lang="ts">
import { computed, ref } from 'vue';
import { useControlFocus } from '../../composables/useControlFocus';
import RelaySwitch from './RelaySwitch.vue';
import RelayPublication from './RelayPublication.vue';
import type { RelayPublicationModel } from '../../composables/useRelayPublication';
import type { RelayModel } from '../../composables/useRelay';
import { relayOutcome } from '../../services/relay';
import '../../styles/relay.css';
const props = defineProps<{
  model: RelayModel;
  signedIn: boolean;
  publication: RelayPublicationModel;
  epoch: string;
}>();
const refreshButton = ref<HTMLButtonElement>();
const recoveryButton = ref<HTMLButtonElement>();
const fallbackFocus = () =>
  refreshButton.value?.matches(':disabled') ? recoveryButton.value : refreshButton.value;
const controlFocus = useControlFocus(() => props.epoch, fallbackFocus);
const status = computed(() => {
  if (!props.signedIn) return '登录同一个账号，接续另一台设备的播放。';
  if (props.model.loading.value) return '正在检查其他设备…';
  switch (props.model.page.value?.availability) {
    case 'available':
      return props.model.page.value.alert ? '发现可接续的播放内容' : '暂时没有其他设备的接续记录';
    case 'experiment_excluded':
      return '服务端尚未向当前账号开放跨端续播';
    case 'server_disabled':
      return '当前设备的跨端续播未开启';
    default:
      return '尚未检查其他设备';
  }
});
</script>
<template>
  <section
    class="settings-section relay-settings"
    aria-labelledby="settings-relay"
    :aria-busy="model.busy.value || model.loading.value"
  >
    <h2 id="settings-relay">跨端续播</h2>
    <div class="settings-section-content">
      <div class="settings-read-status">
        <span role="status">{{ status }}</span
        ><button
          ref="refreshButton"
          v-if="signedIn"
          class="secondary"
          aria-label="检查其他设备的播放"
          :disabled="model.busy.value || model.loading.value || !!model.unknown.value"
          @click="controlFocus.run($event, () => model.read(true))"
        >
          检查其他设备
        </button>
      </div>
      <RelaySwitch :model="model" :epoch="epoch" :fallback-focus="fallbackFocus" />
      <p class="settings-note">歌曲从头播放，播客接续上次进度。接续会替换本机当前队列。</p>
      <p v-if="model.error.value" class="relay-error" role="alert">{{ model.error.value }}</p>
      <div v-if="model.unknown.value" class="relay-recovery">
        <p>尚未收到操作结果，请先核对，避免重复接续。</p>
        <button
          ref="recoveryButton"
          class="secondary"
          :disabled="model.busy.value"
          @click="controlFocus.run($event, model.cancelUnknown)"
        >
          核对操作结果
        </button>
      </div>
      <div v-if="model.page.value?.alert" class="relay-settings-device">
        <div>
          <strong>{{ model.page.value.alert.deviceName }}</strong>
          <p>{{ model.page.value.alert.resourceName }}</p>
        </div>
        <button
          class="primary"
          :disabled="
            model.busy.value || model.loading.value || !!model.unknown.value || !!model.error.value
          "
          @click="controlFocus.run($event, model.continueHere)"
        >
          继续播放
        </button>
      </div>
      <div
        v-for="operation in model.page.value?.operations.slice(0, 5) ?? []"
        :key="operation.id"
        class="relay-operation"
      >
        <span>{{ relayOutcome(operation) }}</span
        ><button
          v-if="operation.canRetry"
          class="secondary"
          :disabled="model.busy.value || !!model.unknown.value"
          @click="controlFocus.run($event, () => model.retry(operation.id))"
        >
          重新准备播放
        </button>
      </div>
      <RelayPublication v-if="signedIn" :model="publication" :epoch="epoch" />
    </div>
  </section>
</template>
