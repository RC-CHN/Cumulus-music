<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, shallowRef } from 'vue';
import { useCore } from '../../services/core';
import type { LocalNetworkReport, LoopbackCheck } from '../../services/networkDiagnostics';
import Icon from '../Icon.vue';

const emit = defineEmits<{ close: [] }>();
const core = useCore();
const dialog = ref<HTMLDialogElement>();
const report = shallowRef<LocalNetworkReport>();
const state = ref<'running' | 'finished' | 'failed'>('running');
let active = true;
const loopbackLabels: Record<LoopbackCheck, string> = {
  available: '可用',
  permission_denied: '权限受限',
  unavailable: '不可用',
};
const routeLabels: Record<LocalNetworkReport['defaultRoute'], string> = {
  present: '已检测到',
  missing: '未检测到',
  unavailable: '无法读取',
};

async function run() {
  state.value = 'running';
  report.value = undefined;
  try {
    const checked = await core.localNetworkDiagnostics();
    if (!active) return;
    report.value = checked;
    state.value = 'finished';
  } catch {
    if (active) state.value = 'failed';
  }
}
function close() {
  active = false;
  emit('close');
}
onMounted(() => {
  dialog.value?.showModal();
  void run();
});
onBeforeUnmount(() => (active = false));
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog settings-network-diagnostics"
    aria-labelledby="settings-network-diagnostics-title"
    aria-describedby="settings-network-diagnostics-scope"
    @cancel.prevent="close"
  >
    <header>
      <h2 id="settings-network-diagnostics-title">网络诊断</h2>
      <button class="icon-button" type="button" aria-label="关闭网络诊断" @click="close">
        <Icon name="close" />
      </button>
    </header>
    <p id="settings-network-diagnostics-scope">诊断仅在本机运行，不访问外部服务，不上传结果。</p>
    <p v-if="state === 'running'" class="network-diagnostics-progress" role="status">
      <span class="spinner" aria-hidden="true" /> 正在检查本机网络…
    </p>
    <p v-else-if="state === 'failed'" class="catalog-error" role="alert">未能完成诊断，请重试。</p>
    <template v-else-if="report">
      <p role="status">本机检查完成。</p>
      <dl class="network-diagnostics-results">
        <dt>IPv4 本机连接</dt>
        <dd>{{ loopbackLabels[report.ipv4Loopback] }}</dd>
        <dt>IPv6 本机连接</dt>
        <dd>{{ loopbackLabels[report.ipv6Loopback] }}</dd>
        <dt>默认网络路由</dt>
        <dd>{{ routeLabels[report.defaultRoute] }}</dd>
        <dt>代理环境变量</dt>
        <dd>{{ report.proxyEnvironmentConfigured ? '已配置' : '未检测到' }}</dd>
        <dt>云音乐主站</dt>
        <dd>未检查</dd>
      </dl>
      <p class="settings-note">
        本机检查无法判断防火墙、系统代理或云音乐主站是否可用。若仍无法连接，请检查系统网络或切换网络后重试。
      </p>
    </template>
    <footer>
      <button v-if="state !== 'running'" class="secondary" type="button" @click="run">
        重新诊断
      </button>
      <button class="primary" type="button" autofocus @click="close">
        {{ state === 'running' ? '取消诊断' : '完成' }}
      </button>
    </footer>
  </dialog>
</template>
