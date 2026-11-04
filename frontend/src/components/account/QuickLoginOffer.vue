<script setup lang="ts">
import { onMounted, onUnmounted, ref, shallowRef } from 'vue';
import type { QuickLoginOffer } from '../../services/auth';
import { failure } from '../../services/contracts';
import { useCore } from '../../services/core';

const props = defineProps<{ epoch: string }>();
const emit = defineEmits<{ accounts: [] }>();
const core = useCore();
const offer = shallowRef<QuickLoginOffer>();
const busy = ref(false);
const notice = ref('');
const help = ref(false);
const helpButton = ref<HTMLButtonElement>();
let alive = true;
const epoch = props.epoch;
function current() {
  return alive && props.epoch === epoch;
}
async function load() {
  const value = await core.quickLoginAccess(epoch);
  if (current() && value.epoch === epoch) offer.value = value.value.offer ?? undefined;
}
async function change() {
  const before = offer.value;
  if (!current() || !before || busy.value) return;
  busy.value = true;
  notice.value = '';
  try {
    const result = await core.setQuickLoginOffer(epoch, !before.enabled, before.revision);
    if (current() && result.epoch === epoch) offer.value = result.value;
  } catch (error) {
    if (!current()) return;
    notice.value = failure(error).message;
    // A lost receipt is read back, never replayed.
    try {
      await load();
    } catch {
      if (current()) offer.value = undefined;
    }
  } finally {
    if (current()) busy.value = false;
  }
}
function closeHelp() {
  if (!help.value) return;
  help.value = false;
  helpButton.value?.focus({ preventScroll: true });
}
onMounted(() => void load().catch(() => {}));
onUnmounted(() => {
  alive = false;
});
</script>

<template>
  <div v-if="offer" class="quick-login-offer" data-testid="quick-login-offer">
    <h3>快速登录</h3>
    <div class="quick-login-offer-actions">
      <label>
        <input
          type="checkbox"
          :checked="offer.enabled"
          :aria-disabled="busy"
          @click.prevent="change"
        />
        开启快速登录
      </label>
      <button
        ref="helpButton"
        type="button"
        class="text-button"
        aria-label="快速登录说明"
        :aria-expanded="help"
        @click="help = !help"
        @keydown.esc.stop="closeHelp"
      >
        说明
      </button>
      <button
        v-if="offer.enabled"
        type="button"
        class="text-button"
        :disabled="busy"
        @click="emit('accounts')"
      >
        切换账号列表 ›
      </button>
    </div>
    <p v-if="help" class="settings-note">
      允许此账号参与本机的服务端快速登录和账号切换。关闭不影响本机恢复或启动自动登录。
      主动退出仍会清除当前会话的凭据。
    </p>
    <p v-if="busy" class="settings-note" role="status">正在核对快速登录设置…</p>
    <p v-else-if="notice" class="settings-note" role="alert">{{ notice }}</p>
  </div>
</template>

<style scoped>
.quick-login-offer {
  margin-block-start: 18px;
}
.quick-login-offer h3 {
  margin: 0 0 10px;
  font-size: 14px;
  font-weight: 600;
}
.quick-login-offer-actions,
.quick-login-offer-actions label {
  display: flex;
  align-items: center;
  gap: 8px;
}
.quick-login-offer-actions {
  flex-wrap: wrap;
  gap: 12px;
}
.quick-login-offer-actions [aria-disabled='true'] {
  cursor: wait;
}
</style>
