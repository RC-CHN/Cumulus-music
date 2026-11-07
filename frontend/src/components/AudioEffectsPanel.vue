<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, useId, watch } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import type { AccountView } from '../services/contracts';
import Icon from './Icon.vue';
import EffectDirectory from './EffectDirectory.vue';
import EffectDeviceDirectory from './EffectDeviceDirectory.vue';
import EqualizerPanel from './EqualizerPanel.vue';

const props = defineProps<{ account: AccountView; player: PlayerModel }>();
const emit = defineEmits<{ close: []; login: [] }>();
const routes = [
  { key: 'catalogue', label: '音效' },
  { key: 'devices', label: '设备适配' },
  { key: 'equalizer', label: '均衡器' },
] as const;
type Tab = (typeof routes)[number]['key'];
const active = ref<Tab>('catalogue');
const online = ref(navigator.onLine);
const view = props.player.view;
const title = useId();
const dialog = ref<HTMLDialogElement>();
const tablist = ref<HTMLElement>();
const activeRoute = computed(() => routes.find((route) => route.key === active.value)!);
const tabId = (key: Tab) => `${title}-tab-${key}`;
const panelId = (key: Tab) => `${title}-panel-${key}`;

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
onMounted(() => {
  updateNetwork();
  dialog.value?.showModal();
  focusTab();
  window.addEventListener('online', updateNetwork);
  window.addEventListener('offline', updateNetwork);
});
onUnmounted(() => {
  window.removeEventListener('online', updateNetwork);
  window.removeEventListener('offline', updateNetwork);
});
</script>

<template>
  <dialog
    ref="dialog"
    class="audio-effects-panel"
    :aria-labelledby="title"
    @cancel.prevent="emit('close')"
  >
    <header class="audio-effects-header">
      <h2 :id="title" class="sr-only">音效</h2>
      <button type="button" class="icon-button" aria-label="关闭音效" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
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
            :account="account"
            :binding-status="view?.boundParametric"
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
.audio-effects-panel nav {
  display: flex;
  flex: none;
  gap: 24px;
  padding-inline: 24px;
  border-block-end: 1px solid var(--line);
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
  .audio-effects-content {
    padding-inline: 14px;
  }
}
</style>
