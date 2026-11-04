<script setup lang="ts">
import { ref, watch } from 'vue';
import { failure, type AccountView } from '../../services/contracts';
import { useCore } from '../../services/core';
import { useAccountRead } from '../../composables/useAccountRead';
import { useControlFocus } from '../../composables/useControlFocus';
import ModalTransition from '../ModalTransition.vue';
import SettingsConfirm from './SettingsConfirm.vue';

const props = defineProps<{ account: AccountView }>();
const emit = defineEmits<{ expired: [] }>();
const core = useCore();
const owner = () => (props.account.status === 'authenticated' ? props.account.epoch : null);
const { value, state, error, refresh } = useAccountRead(
  owner,
  (epoch) => core.historySyncPreference(epoch, { kind: 'read' }),
  undefined,
  () => emit('expired'),
);
const checkbox = ref<HTMLInputElement>();
const heading = ref<HTMLElement>();
const focus = useControlFocus(owner, () => checkbox.value ?? heading.value);
const confirmOff = ref(false);
const saving = ref(false);
const saveError = ref('');
const saved = ref(false);
watch(
  owner,
  () => {
    confirmOff.value = false;
    saving.value = false;
    saved.value = false;
    saveError.value = '';
  },
  { flush: 'sync' },
);

async function save(enabled: boolean, confirmedFirstDisable: boolean) {
  const epoch = owner();
  if (!epoch || saving.value || !value.value || state.value !== 'ready') return;
  saved.value = false;
  saveError.value = '';
  saving.value = true;
  try {
    const next = await core.historySyncPreference(epoch, {
      kind: 'set',
      enabled,
      confirmed_first_disable: confirmedFirstDisable,
    });
    if (owner() !== epoch) return;
    if (next.epoch !== epoch || next.value.enabled !== enabled)
      throw { code: 'invalid_response', message: '设置结果不匹配，请重新读取。' };
    value.value = next.value;
    saved.value = true;
  } catch (cause) {
    if (owner() !== epoch) return;
    const issue = failure(cause);
    saveError.value = issue.message;
    // A missing IPC result could have committed locally. Read the stored state
    // once before showing another enabled control, without resubmitting.
    await refresh();
  } finally {
    if (owner() === epoch) {
      saving.value = false;
      confirmOff.value = false;
    }
  }
}

function select(event: Event) {
  if (!value.value || saving.value || state.value !== 'ready') return;
  if (value.value.enabled && !value.value.firstDisableConfirmed) {
    confirmOff.value = true;
    return;
  }
  const desired = !value.value.enabled;
  void focus.run(event, () => save(desired, false));
}
</script>

<template>
  <section
    class="settings-section"
    aria-labelledby="settings-recent-history"
    :aria-busy="saving || state === 'loading'"
  >
    <h2 id="settings-recent-history" ref="heading" tabindex="-1">最近播放记录</h2>
    <div class="settings-section-content">
      <label class="settings-checkbox">
        <input
          ref="checkbox"
          type="checkbox"
          name="recent-history-sync"
          :checked="value?.enabled === true"
          :disabled="saving || state !== 'ready' || !value"
          @click.prevent="select"
        />
        <span>开启后，同步当前账号在各设备的最近播放记录</span>
      </label>
      <p class="settings-note">
        {{
          state === 'signed-out'
            ? '登录后可设置是否显示多端记录。'
            : '只更改当前账号在这台设备上的显示；关闭后仅显示本地记录。'
        }}
      </p>
      <p v-if="saving || state === 'loading' || saved" class="settings-note" role="status">
        {{ saving ? '正在保存…' : state === 'loading' ? '正在读取…' : '已保存。' }}
      </p>
      <p v-if="saveError || error" class="catalog-error" role="alert">{{ saveError || error }}</p>
      <button
        v-if="state === 'error'"
        class="secondary"
        type="button"
        @click="focus.run($event, refresh)"
      >
        重新读取最近播放设置
      </button>
    </div>
  </section>
  <ModalTransition :return-focus="() => checkbox ?? null" :fallback-focus="() => heading ?? null">
    <SettingsConfirm
      v-if="confirmOff"
      title="播放列表同步"
      description="关闭后，本设备最近播放列表仅展示本地记录"
      confirm-label="确定关闭"
      hide-cancel
      :busy="saving"
      error=""
      @close="confirmOff = false"
      @confirm="save(false, true)"
    />
  </ModalTransition>
</template>
