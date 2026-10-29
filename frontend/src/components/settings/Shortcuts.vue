<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import {
  keyInput,
  shortcutLabels,
  shortcutOrder,
  shortcutText,
  shortcutStatusText,
  type ShortcutAction,
  type ShortcutScope,
} from '../../services/shortcuts';
import { useShortcutSettings } from '../../composables/useShortcutSettings';
import { useShortcutStatus } from '../../composables/useShortcutStatus';
import { useControlFocus } from '../../composables/useControlFocus';
import '../../styles/shortcuts.css';
const props = defineProps<{ account: AccountView }>();
const model = useShortcutSettings(),
  native = useShortcutStatus();
const refresh = ref<HTMLButtonElement>(),
  recorder = ref<HTMLInputElement[]>();
const focus = useControlFocus(
  () => props.account.epoch,
  () => refresh.value,
);
const rows = computed(() =>
  shortcutOrder.filter((action) => model.view.value?.current?.bindings[action]),
);
const recordingReady = computed(
  () =>
    !!model.draft.value?.id &&
    native.view.value?.phase === 'recording' &&
    native.view.value.recording === model.draft.value.id,
);
const mediaNames: Record<string, string> = {
  media_play_pause: '播放／暂停',
  media_next: '下一首',
  media_previous: '上一首',
  media_stop: '停止',
};
const scopes: ShortcutScope[] = ['local', 'global'];
const scopeText = (scope: ShortcutScope) => (scope === 'local' ? '窗口内' : '全局');
const row = (action: ShortcutAction, scope: ShortcutScope) =>
  native.view.value?.rows.find(
    (r) => r.scope === scope && r.hotkey.kind === 'configured' && r.hotkey.action === action,
  );
const unavailable = (action: ShortcutAction, scope: ShortcutScope) =>
  row(action, scope)?.status.kind === 'unavailable' &&
  (row(action, scope)!.status as { code: number }).code === 1;
const disabled = (action: ShortcutAction, scope: ShortcutScope) =>
  !model.canChange.value ||
  native.view.value?.phase !== 'ready' ||
  unavailable(action, scope) ||
  (scope === 'global' && !model.view.value?.current?.global_enabled);
const same = (action: ShortcutAction, scope: ShortcutScope) =>
  model.draft.value?.action === action && model.draft.value.scope === scope;
watch(
  () => props.account.epoch,
  () => void model.connect(props.account),
  { immediate: true },
);
watch(native.view, (value) => {
  const id = model.draft.value?.id;
  // Only an externally ended recording removes the focused editor here.
  // Routine registration events must not supersede the save action's focus
  // recovery while its fallback button is still disabled by pending IPC.
  if (!id || !value || value.phase === 'starting' || value.recording === id) return;
  const restore = focus.capture(document.activeElement);
  model.observeNative(value);
  void nextTick().then(restore);
});
watch(recordingReady, async (ready) => {
  if (ready) {
    await nextTick();
    recorder.value?.[0]?.focus({ preventScroll: true });
  }
});
onMounted(native.connect);
onUnmounted(() => {
  model.dispose();
  native.dispose();
});
async function read() {
  await Promise.all([model.read(), native.read()]);
}
function keydown(event: KeyboardEvent) {
  if (event.key === 'Tab') return;
  event.preventDefault();
  event.stopPropagation();
  if (event.key === 'Escape') {
    void focus.run(event, model.cancelRecording);
    return;
  }
  const input = keyInput(event);
  if (
    !recordingReady.value ||
    event.repeat ||
    event.isComposing ||
    [16, 17, 18, 91, 92, 93].includes(input.keyCode)
  )
    return;
  void model.capture(input);
}
async function begin(action: ShortcutAction, scope: ShortcutScope, event: Event) {
  await focus.run(event, () => model.beginRecording(action, scope));
}
</script>
<template>
  <section
    class="settings-section shortcut-settings"
    :data-phase="native.view.value?.phase"
    aria-labelledby="settings-shortcuts"
    :aria-busy="model.loading.value || model.busy.value"
  >
    <h2 id="settings-shortcuts">快捷键</h2>
    <div class="settings-section-content">
      <div class="settings-read-status">
        <span>用键盘控制音乐</span>
        <button
          ref="refresh"
          class="text-button"
          aria-label="重新读取快捷键设置"
          :disabled="model.loading.value || model.busy.value || !!model.draft.value"
          @click="focus.run($event, read)"
        >
          刷新
        </button>
      </div>
      <p
        v-if="(!model.draft.value && model.error.value) || native.error.value"
        class="catalog-error"
        role="alert"
      >
        {{ model.error.value || native.error.value }}
      </p>
      <template v-if="model.view.value?.current">
        <div class="shortcut-switches">
          <label class="settings-checkbox"
            ><input
              type="checkbox"
              name="shortcuts-global"
              :checked="model.view.value.current.global_enabled"
              :disabled="!model.canChange.value"
              @click.prevent="
                focus.run($event, () =>
                  model.change({
                    kind: 'global',
                    enabled: !model.view.value!.current!.global_enabled,
                  }),
                )
              "
            /><span>启用全局快捷键</span></label
          >
          <label class="settings-checkbox"
            ><input
              type="checkbox"
              name="shortcuts-media"
              :checked="model.view.value.current.media_enabled"
              :disabled="!model.canChange.value"
              @click.prevent="
                focus.run($event, () =>
                  model.change({
                    kind: 'media',
                    enabled: !model.view.value!.current!.media_enabled,
                  }),
                )
              "
            /><span>启用键盘媒体键</span></label
          >
        </div>
        <p class="settings-note">保存在这台设备。窗口内快捷键在输入文字或打开弹窗时暂停。</p>
        <div class="shortcut-table" role="table" aria-label="快捷键设置">
          <div class="shortcut-row shortcut-head" role="row">
            <span role="columnheader">功能</span><span role="columnheader">窗口内</span
            ><span role="columnheader">全局</span>
          </div>
          <div
            v-for="action in rows"
            :key="action"
            class="shortcut-row"
            role="row"
            :data-action="action"
          >
            <span role="rowheader">{{ shortcutLabels[action] }}</span>
            <div
              v-for="scope in scopes"
              :key="scope"
              role="cell"
              class="shortcut-cell"
              :data-scope="scope"
            >
              <template v-if="same(action, scope)">
                <input
                  ref="recorder"
                  class="shortcut-recorder"
                  readonly
                  :aria-label="`录入${shortcutLabels[action]}${scopeText(scope)}快捷键`"
                  :value="shortcutText(model.draft.value?.keys)"
                  :disabled="!recordingReady || model.busy.value"
                  @keydown="keydown"
                />
                <div class="shortcut-edit-actions">
                  <button
                    class="text-button"
                    :disabled="!recordingReady || !model.draft.value?.changed || model.busy.value"
                    @click="focus.run($event, model.saveRecording)"
                  >
                    保存</button
                  ><button
                    class="text-button"
                    :disabled="model.busy.value"
                    @click="focus.run($event, model.cancelRecording)"
                  >
                    取消
                  </button>
                </div>
                <p
                  v-if="model.recordError.value || model.error.value"
                  class="catalog-error shortcut-edit-error"
                  role="alert"
                >
                  {{ model.recordError.value || model.error.value }}
                </p>
              </template>
              <button
                v-else
                class="shortcut-binding"
                :aria-label="`修改${shortcutLabels[action]}${scopeText(scope)}快捷键`"
                :disabled="disabled(action, scope)"
                @click="begin(action, scope, $event)"
              >
                {{ shortcutText(model.view.value.current.bindings[action]?.[scope]) }}
              </button>
              <span
                class="shortcut-result"
                :data-state="row(action, scope)?.status.kind ?? 'disabled'"
                >{{ shortcutStatusText(row(action, scope)) }}</span
              >
            </div>
          </div>
        </div>
        <p v-if="model.draft.value" class="settings-note" role="status">
          {{
            recordingReady
              ? '请按下组合键；Backspace 清除，Escape 取消。录入期间暂停全局快捷键。'
              : '正在准备按键录入…'
          }}
        </p>
        <button
          v-if="native.view.value?.recording && !model.draft.value"
          class="secondary"
          @click="focus.run($event, () => model.resumeRecording(native.view.value!.recording!))"
        >
          结束按键录入并恢复快捷键
        </button>
        <div
          v-if="model.view.value.current.media_enabled"
          class="shortcut-media-status"
          aria-label="媒体键状态"
        >
          <span
            v-for="item in native.view.value?.rows.filter((r) =>
              r.hotkey.kind.startsWith('media_'),
            ) ?? []"
            :key="item.hotkey.kind"
            >{{ mediaNames[item.hotkey.kind] }}：{{ shortcutStatusText(item) }}</span
          >
        </div>
        <div class="shortcut-footer">
          <span class="settings-note">空格与 F5：播放／暂停</span
          ><button
            class="text-button"
            :disabled="!model.canChange.value"
            @click="focus.run($event, () => model.change({ kind: 'reset' }))"
          >
            恢复默认快捷键
          </button>
        </div>
      </template>
      <div v-if="model.view.value?.damaged" class="settings-recovery" role="alert">
        <p>本机快捷键设置已损坏，已暂停快捷键。恢复默认值后可重新设置。</p>
        <button
          class="secondary"
          :disabled="model.loading.value || model.busy.value"
          @click="focus.run($event, model.repair)"
        >
          恢复默认快捷键设置
        </button>
      </div>
      <p v-if="model.loading.value || model.busy.value" class="settings-note" role="status">
        {{ model.loading.value ? '正在读取…' : '正在保存…' }}
      </p>
      <p v-else-if="model.saved.value" class="settings-note" role="status">已保存。</p>
      <p v-if="native.view.value?.phase === 'not_installed'" class="settings-note">
        此环境暂不提供原生快捷键。
      </p>
    </div>
  </section>
</template>
