<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue';
import { useCore } from '../../services/core';
import { updateFailure, type UpdateCheck } from '../../services/updates';
import Icon from '../Icon.vue';

const emit = defineEmits<{ close: [] }>();
const core = useCore();
const dialog = ref<HTMLDialogElement>();
const report = shallowRef<UpdateCheck>();
const checking = ref(true);
const opening = ref(false);
const error = ref('');
let active = true;
const heading = computed(() => (report.value?.state === 'available' ? '发现新版本' : '检查更新'));

async function check() {
  report.value = undefined;
  error.value = '';
  checking.value = true;
  try {
    const value = await core.checkForUpdates();
    if (active) report.value = value;
  } catch (reason) {
    if (active) error.value = updateFailure(reason);
  } finally {
    if (active) checking.value = false;
  }
}
async function openRelease() {
  if (report.value?.state !== 'available' || opening.value) return;
  opening.value = true;
  error.value = '';
  try {
    await core.openUpdateReleasePage();
    if (active) close();
  } catch (reason) {
    if (active) {
      if (reason === 'no_offer') report.value = undefined;
      error.value = updateFailure(reason);
    }
  } finally {
    if (active) opening.value = false;
  }
}
function close() {
  active = false;
  emit('close');
}
onMounted(() => {
  dialog.value?.showModal();
  void check();
});
onBeforeUnmount(() => (active = false));
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog settings-update-check"
    aria-labelledby="settings-update-check-title"
    @cancel.prevent="!opening && close()"
  >
    <header>
      <h2 id="settings-update-check-title">{{ heading }}</h2>
      <button
        class="icon-button"
        type="button"
        aria-label="关闭更新检查"
        :disabled="opening"
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <p v-if="checking" class="network-diagnostics-progress" role="status">
      <span class="spinner" aria-hidden="true" /> 正在检查公开稳定版…
    </p>
    <template v-else-if="report">
      <p v-if="report.state === 'available'" role="status">
        已发布 Cumulus {{ report.latestVersion }}，当前版本为 {{ report.currentVersion }}。
      </p>
      <p v-else-if="report.state === 'no_newer'" role="status">
        {{
          report.currentVersion === report.latestVersion
            ? `您当前使用的是最新公开稳定版（${report.currentVersion}）。`
            : `未发现比当前版本 ${report.currentVersion} 更新的公开稳定版。`
        }}
      </p>
      <p v-else role="status">GitHub 暂未返回可检查的公开稳定版。</p>
      <p v-if="report.state === 'available'" class="settings-note">
        将在系统浏览器打开 Cumulus 的 GitHub 发布页。下载和安装由你决定。
      </p>
    </template>
    <p v-if="error" role="alert" class="catalog-error">{{ error }}</p>
    <footer>
      <button class="secondary" type="button" autofocus :disabled="opening" @click="close">
        {{ checking ? '关闭' : report?.state === 'available' ? '稍后' : '知道了' }}
      </button>
      <button
        v-if="report?.state === 'available'"
        class="primary"
        type="button"
        :disabled="opening"
        @click="openRelease"
      >
        {{ opening ? '正在打开…' : '查看发布页' }}
      </button>
      <button v-else-if="!checking && error" class="primary" type="button" @click="check">
        重试
      </button>
    </footer>
  </dialog>
</template>
