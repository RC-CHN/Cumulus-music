<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { useCore } from '../../services/core';
import ModalTransition from '../ModalTransition.vue';
import NetworkDiagnosticsDialog from './NetworkDiagnosticsDialog.vue';

const core = useCore();
const opener = ref<HTMLButtonElement>();
const open = ref(false);
const version = ref('');
onMounted(async () => {
  try {
    version.value = (await core.getAppInfo()).coreVersion;
  } catch {
    // The diagnostics entry is available even when version metadata is unavailable.
  }
});
</script>

<template>
  <section class="settings-section" aria-labelledby="settings-about">
    <h2 id="settings-about">关于 Cumulus</h2>
    <div class="settings-section-content">
      <p v-if="version">当前版本 {{ version }}</p>
      <p class="settings-note">检查本机的网络设置，不连接外部服务，也不上传诊断结果。</p>
      <button ref="opener" class="secondary" type="button" @click="open = true">网络诊断</button>
    </div>
    <ModalTransition :return-focus="() => opener ?? null">
      <NetworkDiagnosticsDialog v-if="open" @close="open = false" />
    </ModalTransition>
  </section>
</template>
