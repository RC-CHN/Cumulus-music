<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, useId, watch } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { useHeadphoneHeader } from '../composables/useHeadphoneHeader';
import { useCore } from '../services/core';
import { headphoneTuningLabel } from '../services/effectDirectory';
import type { AccountView } from '../services/contracts';
import type { EffectCataloguePresentation } from '../services/pcPresentation';
import { equalizerTone, equalizerToneEnabled } from '../services/equalizer';
import Icon from './Icon.vue';
import EffectDirectory from './EffectDirectory.vue';
import EffectDeviceDirectory from './EffectDeviceDirectory.vue';
import EqualizerPanel from './EqualizerPanel.vue';
import LocalEqualizerMaster from './LocalEqualizerMaster.vue';
import EffectMasterSwitch from './EffectMasterSwitch.vue';

const props = defineProps<{
  account: AccountView;
  player: PlayerModel;
  effectCatalogue?: EffectCataloguePresentation;
}>();
const emit = defineEmits<{ close: []; login: [] }>();
const routes = [
  { key: 'catalogue', label: '音效' },
  { key: 'devices', label: '设备适配' },
  { key: 'equalizer', label: '均衡器' },
] as const;
type Tab = (typeof routes)[number]['key'];
const active = ref<Tab>('catalogue');
const online = ref(navigator.onLine);
const { view, ready } = props.player;
const headphone = useHeadphoneHeader(
  useCore(),
  () => props.account,
  () => (ready.value ? view.value : undefined),
  () => props.effectCatalogue?.layout !== 'legacy',
);
const {
  visible: showHeadphoneMaster,
  target: headphoneTarget,
  pending: headphonePending,
  error: headphoneError,
} = headphone;
const title = useId();
const dialog = ref<HTMLDialogElement>();
const tablist = ref<HTMLElement>();
const activeRoute = computed(() => routes.find((route) => route.key === active.value)!);
const tabId = (key: Tab) => `${title}-tab-${key}`;
const panelId = (key: Tab) => `${title}-panel-${key}`;
const selectedLocalEqualizer = computed(() => {
  const current = view.value;
  const eq = current?.equalizer;
  if (
    !ready.value ||
    !current ||
    current.epoch !== props.account.epoch ||
    current.deviceParametric ||
    current.boundParametric ||
    !eq
  )
    return;
  // An open master bit is the initial local default, not a selected effect.
  // An explicitly enabled flat GEQ is still a selection; saved gains alone are not.
  const { bass_db, treble_db } = equalizerTone(eq.settings);
  if (
    eq.settings.enabled ||
    (equalizerToneEnabled(eq.settings) && (bass_db !== 0 || treble_db !== 0))
  )
    return eq;
});
const showLocalMaster = computed(() => {
  const eq = selectedLocalEqualizer.value;
  return (
    !!eq &&
    !eq.damaged &&
    !eq.source_bypassed &&
    (eq.status === 'configured' || eq.status === 'processing')
  );
});
const canToggleLocalMaster = () => showLocalMaster.value;
const localMasterIssue = computed(
  () =>
    active.value !== 'equalizer' &&
    !!selectedLocalEqualizer.value &&
    (props.player.equalizerDraft.blocked.value || selectedLocalEqualizer.value.status === 'failed'),
);

function focusTab() {
  tablist.value?.querySelector<HTMLButtonElement>('[aria-selected="true"]')?.focus({
    preventScroll: true,
  });
}
async function activate(key: Tab) {
  active.value = key;
  await nextTick();
  if (dialog.value?.open && active.value === key) focusTab();
}
function keyboard(event: KeyboardEvent) {
  if (!(event.target instanceof HTMLButtonElement) || event.target.getAttribute('role') !== 'tab')
    return;
  const index = routes.indexOf(activeRoute.value);
  let next: number;
  switch (event.key) {
    case 'ArrowLeft':
      next = (index + routes.length - 1) % routes.length;
      break;
    case 'ArrowRight':
      next = (index + 1) % routes.length;
      break;
    case 'Home':
      next = 0;
      break;
    case 'End':
      next = routes.length - 1;
      break;
    default:
      return;
  }
  if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
  event.preventDefault();
  void activate(routes[next]!.key);
}
function updateNetwork() {
  const next = navigator.onLine;
  // Disconnecting may remove a focused catalogue control. Return to its
  // owning tab only if that control was actually inside the pane.
  const focused = document.activeElement;
  const restoreFocus =
    active.value === 'catalogue' &&
    online.value !== next &&
    focused?.closest('[data-audio-effect-pane="catalogue"]');
  online.value = next;
  if (restoreFocus)
    void nextTick(() => {
      if (
        dialog.value?.open &&
        active.value === 'catalogue' &&
        (document.activeElement === focused || document.activeElement === document.body)
      )
        focusTab();
    });
}
watch(
  () => [props.account.epoch, props.account.status],
  () => emit('close'),
);
watch(showLocalMaster, async (visible) => {
  const focused = document.activeElement;
  if (visible || !focused?.closest('.audio-effects-master')) return;
  await nextTick();
  if (dialog.value?.open && document.activeElement === document.body) focusTab();
});
watch(showHeadphoneMaster, async (visible) => {
  const focused = document.activeElement;
  const losingHeader = !visible && focused?.closest('.audio-effects-headphone-master');
  const movingFromDevice =
    visible && focused?.closest('.effect-device-active [data-comparison-receipt]');
  if (!losingHeader && !movingFromDevice) return;
  await nextTick();
  if (
    !dialog.value?.open ||
    (document.activeElement !== document.body && document.activeElement !== dialog.value)
  )
    return;
  if (movingFromDevice)
    dialog.value.querySelector<HTMLButtonElement>('.audio-effects-headphone-master')?.focus();
  else focusTab();
});
onMounted(() => {
  updateNetwork();
  dialog.value?.showModal();
  focusTab();
  window.addEventListener('online', updateNetwork);
  window.addEventListener('offline', updateNetwork);
});
onUnmounted(() => {
  headphone.dispose();
  window.removeEventListener('online', updateNetwork);
  window.removeEventListener('offline', updateNetwork);
});
</script>

<template>
  <dialog
    ref="dialog"
    class="audio-effects-panel"
    :data-audio-effect-layout="effectCatalogue?.layout ?? 'legacy'"
    :aria-labelledby="title"
    @cancel.prevent="emit('close')"
  >
    <header class="audio-effects-header">
      <h2 :id="title" class="sr-only">音效</h2>
      <button type="button" class="icon-button" aria-label="关闭音效" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
    <div class="audio-effects-tab-bar">
      <nav ref="tablist" role="tablist" aria-label="音效页签" @keydown="keyboard">
        <button
          v-for="route in routes"
          :id="tabId(route.key)"
          :key="route.key"
          type="button"
          role="tab"
          :data-audio-effect-tab="route.key"
          :aria-selected="active === route.key"
          :aria-controls="panelId(route.key)"
          :tabindex="active === route.key ? 0 : -1"
          @click="activate(route.key)"
        >
          {{ route.label }}
        </button>
      </nav>
      <EffectMasterSwitch
        v-if="showHeadphoneMaster && headphoneTarget"
        class="audio-effects-master audio-effects-headphone-master"
        label="耳机调音"
        :checked="headphoneTarget.device.status === 'configured'"
        :busy="!!headphonePending"
        :title="headphoneTuningLabel(headphoneTarget.device.status)"
        :data-comparison-receipt="headphoneTarget.comparison.receipt"
        keep-focus
        @toggle="
          headphone.toggle(($event.currentTarget as HTMLButtonElement).dataset.comparisonReceipt)
        "
      >
        耳机音效
      </EffectMasterSwitch>
      <LocalEqualizerMaster
        v-else-if="showLocalMaster"
        class="audio-effects-master"
        :player="player"
        :can-toggle="canToggleLocalMaster"
        in-header
      />
    </div>
    <p v-if="showHeadphoneMaster && headphoneError" class="audio-effects-master-issue" role="alert">
      {{ headphoneError }}
    </p>
    <p v-if="localMasterIssue" class="audio-effects-master-issue" role="status">
      均衡器设置尚未确认。
      <button type="button" @click="activate('equalizer')">查看均衡器</button>
    </p>
    <section
      v-for="route in routes"
      :id="panelId(route.key)"
      :key="route.key"
      role="tabpanel"
      :aria-labelledby="tabId(route.key)"
      :data-audio-effect-pane="route.key"
      class="audio-effects-content"
      :hidden="active !== route.key"
    >
      <template v-if="active === route.key">
        <template v-if="route.key === 'catalogue'">
          <EffectDirectory
            v-if="online"
            embedded
            :effect-catalogue-revision="effectCatalogue?.revision"
            :account="account"
            :binding-status="view?.boundParametric"
            :applied-effect-id="
              ready && view?.epoch === account.epoch ? view.appliedEffectId : undefined
            "
            :parametric-processing="view?.parametricProcessing"
            @login="emit('login')"
          />
          <p v-else class="audio-effects-message" role="status">当前离线，联网后可查看音效目录。</p>
        </template>
        <div v-else-if="route.key === 'devices'" class="audio-effects-device-content">
          <EffectDeviceDirectory
            v-if="account.status === 'authenticated'"
            :account="account"
            :applied="view?.deviceParametric"
            :parametric-processing="view?.parametricProcessing"
            :catalogue-parametric="view?.boundParametric"
            :comparison-in-header="showHeadphoneMaster"
            :header-comparison-busy="!!headphonePending"
          />
          <div v-else class="audio-effects-message">
            <p>登录后可查询耳机品牌和型号。</p>
            <button type="button" class="primary" @click="emit('login')">登录</button>
          </div>
        </div>
        <EqualizerPanel
          v-else
          embedded
          :account="account"
          :player="player"
          :master-in-header="showLocalMaster"
          @close="emit('close')"
          @login="emit('login')"
        />
      </template>
    </section>
  </dialog>
</template>

<style scoped>
.audio-effects-panel {
  width: min(574px, calc(100vw - 32px));
  height: min(560px, calc(100dvh - 32px));
  max-width: none;
  max-height: none;
  padding: 0;
  overflow: hidden;
  color: var(--text);
  background: var(--surface-raised);
  border: 1px solid var(--line);
  border-radius: 14px;
  box-shadow: 0 18px 70px #0003;
}
.audio-effects-panel[data-audio-effect-layout='legacy'] {
  width: min(768px, calc(100vw - 32px));
  height: min(480px, calc(100dvh - 32px));
}
.audio-effects-panel[open] {
  display: flex;
  flex-direction: column;
}
.audio-effects-panel::backdrop {
  background: #0004;
}
.audio-effects-header {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  min-height: 32px;
  flex: none;
  padding-inline: 24px 14px;
}
.audio-effects-tab-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex: none;
  gap: 12px;
  padding-inline: 24px;
  border-block-end: 1px solid var(--line);
}
.audio-effects-panel nav {
  display: flex;
  gap: 24px;
}
.audio-effects-master-issue {
  margin: 8px 24px 0;
  font-size: 12px;
  color: var(--text-secondary);
}
.audio-effects-master-issue button {
  color: var(--accent);
}
.audio-effects-panel [role='tab'] {
  position: relative;
  min-height: 36px;
  padding-inline: 2px;
  color: var(--text-secondary);
  white-space: nowrap;
}
.audio-effects-panel [aria-selected='true'] {
  color: var(--text);
  font-weight: 600;
}
.audio-effects-panel [aria-selected='true']::after {
  position: absolute;
  inset: auto 2px -1px;
  height: 2px;
  border-radius: 1px;
  background: var(--accent-fill);
  content: '';
}
.audio-effects-content {
  min-height: 0;
  flex: 1 1 auto;
  padding: 14px 24px 20px;
  overflow: hidden;
}
.audio-effects-content:not([hidden]) {
  display: flex;
  flex-direction: column;
}
.audio-effects-device-content {
  overflow-y: auto;
}
.audio-effects-message {
  color: var(--text-secondary);
  margin-block: 12px;
}
@media (max-width: 540px) {
  .audio-effects-tab-bar {
    padding-inline: 14px;
  }
  .audio-effects-panel nav {
    gap: 12px;
  }
  .audio-effects-content {
    padding-inline: 14px;
  }
}
@media (max-width: 380px) {
  .audio-effects-tab-bar {
    flex-wrap: wrap;
    gap: 0 12px;
  }
}
</style>
