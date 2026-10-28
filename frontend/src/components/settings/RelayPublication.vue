<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { RelayPublicationModel } from '../../composables/useRelayPublication';
import { publicationOutcome } from '../../services/relayPublication';
import ModalTransition from '../ModalTransition.vue';
import SettingsConfirm from './SettingsConfirm.vue';
const props = defineProps<{ model: RelayPublicationModel }>();
const confirmation = ref<string>();
const refreshButton = ref<HTMLButtonElement>();
const fallbackFocus = () => refreshButton.value ?? null;
const status = computed(() =>
  props.model.page.value ? publicationOutcome(props.model.page.value) : '尚未读取本机同步状态',
);
watch(
  () => props.model.page.value?.blocked?.id,
  (id) => {
    if (id !== confirmation.value) confirmation.value = undefined;
  },
);
async function confirm() {
  if (confirmation.value) {
    await props.model.resynchronize(confirmation.value);
    confirmation.value = undefined;
  }
}
</script>
<template>
  <div class="relay-publication" :aria-busy="model.busy.value || model.loading.value">
    <div class="settings-read-status">
      <h3>本机播放同步</h3>
      <button
        ref="refreshButton"
        class="secondary"
        :disabled="model.busy.value || model.loading.value"
        @click="model.read()"
      >
        刷新同步状态
      </button>
    </div>
    <p role="status">{{ status }}</p>
    <p v-if="model.page.value?.blocked?.serviceCode" class="settings-note">
      服务返回代码：{{ model.page.value.blocked.serviceCode }}
    </p>
    <p v-if="model.error.value" class="relay-error" role="alert">{{ model.error.value }}</p>
    <div v-if="model.unknown.value" class="relay-recovery">
      <p>尚未收到操作结果，请先核对；尚未开始的请求将取消。</p>
      <button
        class="secondary"
        :disabled="model.busy.value || model.loading.value"
        @click="model.resolveUnknown()"
      >
        核对同步结果
      </button>
    </div>
    <p v-if="model.page.value?.requested?.phase === 'cancelled'" class="settings-note">
      已取消尚未开始的重新同步。
    </p>
    <div class="relay-recovery-actions">
      <button
        v-if="model.page.value?.canResynchronize"
        class="secondary"
        :disabled="!model.canResynchronize.value"
        @click="confirmation = model.page.value?.blocked?.id"
      >
        重新同步当前队列
      </button>
      <button
        v-if="model.page.value?.recovering?.phase === 'prepared'"
        class="secondary"
        :disabled="
          model.busy.value ||
          model.loading.value ||
          !!model.unknown.value ||
          model.page.value?.recordFailed
        "
        @click="model.resumePrepared()"
      >
        重试连接
      </button>
    </div>
    <ModalTransition :fallback-focus="fallbackFocus"
      ><SettingsConfirm
        v-if="confirmation"
        title="重新同步当前队列"
        description="将本机当前队列作为新的续播记录提交，可能替换其他设备最近上传的记录。本机播放不会重新开始。"
        :busy="model.busy.value"
        :error="model.error.value"
        @close="confirmation = undefined"
        @confirm="confirm"
    /></ModalTransition>
  </div>
</template>
