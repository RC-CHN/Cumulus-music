<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue';
import { useCore } from '../../services/core';
import {
  packageFailure,
  updateFailure,
  type PackageVerification,
  type UpdateCheck,
} from '../../services/updates';
import Icon from '../Icon.vue';

const emit = defineEmits<{ close: [] }>();
const core = useCore();
const dialog = ref<HTMLDialogElement>();
const packageButton = ref<HTMLButtonElement>();
const report = shallowRef<UpdateCheck>();
const checking = ref(true);
const opening = ref(false);
const opened = ref(false);
const verifying = ref(false);
const verified = shallowRef<PackageVerification>();
const verificationError = ref('');
const canVerify = computed(() => !!report.value && report.value.state !== 'no_published_release');
const error = ref('');
let active = true;
const heading = computed(() => (report.value?.state === 'available' ? '发现新版本' : '检查更新'));

async function check() {
  report.value = undefined;
  opened.value = false;
  verified.value = undefined;
  verificationError.value = '';
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
  if (report.value?.state !== 'available' || opening.value || opened.value || verifying.value)
    return;
  opening.value = true;
  error.value = '';
  try {
    await core.openUpdateReleasePage();
    if (active) {
      opened.value = true;
      await nextTick();
    }
  } catch (reason) {
    if (active) {
      if (reason === 'no_offer') report.value = undefined;
      error.value = updateFailure(reason);
    }
  } finally {
    if (active) {
      opening.value = false;
      if (opened.value) {
        await nextTick();
        if (active) packageButton.value?.focus({ preventScroll: true });
      }
    }
  }
}
async function verifyPackage() {
  if (!canVerify.value || verifying.value || opening.value) return;
  verificationError.value = '';
  verified.value = undefined;
  verifying.value = true;
  try {
    const outcome = await core.verifyUpdatePackage();
    if (active) verified.value = outcome ?? undefined;
  } catch (reason) {
    if (active) verificationError.value = packageFailure(reason);
  } finally {
    if (active) {
      verifying.value = false;
      await nextTick();
      if (active && (!document.activeElement || document.activeElement === document.body))
        packageButton.value?.focus({ preventScroll: true });
    }
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
    @cancel.prevent="!opening && !verifying && close()"
  >
    <header>
      <h2 id="settings-update-check-title">{{ heading }}</h2>
      <button
        class="icon-button"
        type="button"
        aria-label="关闭更新检查"
        :disabled="opening || verifying"
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
        {{
          opened
            ? '已打开 GitHub 发布页。下载完成后，可在这里校验安装包。'
            : '将在系统浏览器打开 Cumulus 的 GitHub 发布页。'
        }}
      </p>
      <div v-if="canVerify" class="settings-update-package">
        <p class="settings-note">
          可校验已经下载到本机的安装包。校验只比对本次发布的 SHA-256，不会安装或运行文件。
        </p>
        <button
          ref="packageButton"
          class="secondary"
          type="button"
          :disabled="verifying || opening"
          @click="verifyPackage"
        >
          {{ verifying ? '正在校验…' : '校验本地安装包' }}
        </button>
        <p v-if="verified" role="status" class="settings-note">
          {{ verified.filename }} 的 SHA-256 与 {{ verified.version }} 发布记录一致。
        </p>
        <p v-if="verificationError" role="alert" class="catalog-error">{{ verificationError }}</p>
      </div>
    </template>
    <p v-if="error" role="alert" class="catalog-error">{{ error }}</p>
    <footer>
      <button
        class="secondary"
        type="button"
        autofocus
        :disabled="opening || verifying"
        @click="close"
      >
        {{ checking ? '关闭' : report?.state === 'available' && !opened ? '稍后' : '知道了' }}
      </button>
      <button
        v-if="report?.state === 'available' && !opened"
        class="primary"
        type="button"
        :disabled="opening || verifying"
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
