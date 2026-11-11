<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import type { DownloadLayoutField, DownloadLayoutValue } from '../../services/downloads';
import type { DownloadModel } from '../../composables/useDownloads';
import {
  createDownloadDirectorySettings,
  directorySettingsDescription,
  displayDownloadDirectory,
  type DownloadDirectorySettingsOwner,
} from '../../composables/useDownloadDirectorySettings';
import { createDownloadLayoutSettings } from '../../composables/useDownloadLayoutSettings';
import { useControlFocus } from '../../composables/useControlFocus';
import { useCore } from '../../services/core';
import ModalTransition from '../ModalTransition.vue';
import SettingsConfirm from './SettingsConfirm.vue';

const props = defineProps<{
  account: AccountView;
  model: DownloadModel;
  visit: () => object | undefined;
}>();
const emit = defineEmits<{ downloads: []; preferences: []; expired: [] }>();
const core = useCore();
const opener = ref<HTMLButtonElement>();
const manage = ref<HTMLButtonElement>();
const migrationStatus = ref<HTMLElement>();
const layoutControls = ref<HTMLElement>();
let alive = true;
const scope = computed<DownloadDirectorySettingsOwner | undefined>((previous) => {
  const visit = props.visit();
  if (!visit || props.account.status !== 'authenticated') return undefined;
  const epoch = props.account.epoch;
  return previous?.epoch === epoch && previous.visit === visit ? previous : { epoch, visit };
});
const downloads = computed(() => {
  const view = props.model.view.value;
  return view?.epoch === scope.value?.epoch ? view : undefined;
});
const displayedRoot = computed(() => {
  const root = downloads.value?.directory?.root;
  return root ? displayDownloadDirectory(root) : undefined;
});
const readError = computed(() => props.model.readError.value);
const migration = computed(() => downloads.value?.migration);
const blocked = () =>
  !scope.value ||
  !downloads.value ||
  !!readError.value ||
  props.model.busy.value ||
  !!migration.value ||
  layout.activity.value === 'saving';
const directory = createDownloadDirectorySettings({
  owner: () => (alive ? scope.value : undefined),
  blocked,
  pick: (epoch) => core.pickDownloadDirectory(epoch),
  preview: (epoch, path) => core.downloadDirectoryPreview(epoch, path),
  refresh: () => props.model.refresh(),
  commit: (epoch, command) => props.model.changeDirectory(command, epoch),
  expired: () => emit('expired'),
});
const { activity, confirmation, feedback } = directory;
const layout = createDownloadLayoutSettings({
  owner: () => (alive ? scope.value : undefined),
  blocked: () =>
    !!activity.value || !!confirmation.value || !!migration.value || props.model.busy.value,
  read: (epoch) => core.downloadLayout(epoch),
  change: (epoch, change) => core.changeDownloadLayout(epoch, change),
  directoryChanged: () => void props.model.refresh(true),
  expired: () => emit('expired'),
});
const { value: layoutValue, activity: layoutActivity, feedback: layoutFeedback } = layout;
const layoutDisabled = computed(() => !layout.canChoose());
const layoutGroups = [
  {
    field: 'file_type',
    title: '音乐命名格式',
    options: [
      { value: 1, label: '歌曲名' },
      { value: 0, label: '歌手-歌曲名' },
      { value: 2, label: '歌曲名-歌手' },
    ],
  },
  {
    field: 'path_type',
    title: '文件智能分类',
    options: [
      { value: 0, label: '不分文件夹' },
      { value: 1, label: '按歌手分文件夹' },
      { value: 2, label: '按歌手\\专辑分文件夹' },
    ],
  },
] as const;
function changeLayout(event: Event, field: DownloadLayoutField, value: DownloadLayoutValue) {
  void layout.choose(field, value);
  // Native arrows toggle the DOM before change fires. Restore the projected
  // choice if a pending action or failed read blocked this event.
  const selected = layout.shown(field);
  if (selected !== value) {
    const input = event.currentTarget as HTMLInputElement;
    input.checked = false;
    const previous = input
      .closest('fieldset')
      ?.querySelector<HTMLInputElement>(`input[value="${selected}"]`);
    if (previous) previous.checked = true;
  }
}
function guardLayoutKeys(event: KeyboardEvent) {
  if (
    layoutDisabled.value &&
    ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', ' '].includes(event.key)
  )
    event.preventDefault();
}
const focus = useControlFocus(
  () => scope.value,
  () => opener.value,
);
const layoutFocus = useControlFocus(
  () => scope.value,
  () => layoutControls.value?.querySelector<HTMLInputElement>('input:checked'),
);
function checkLayout(event: MouseEvent) {
  if (!layoutActivity.value) void layoutFocus.run(event, () => layout.refresh());
}
const repick = shallowRef<DownloadDirectorySettingsOwner>();
const dialogOwner = shallowRef<DownloadDirectorySettingsOwner>();
watch(
  confirmation,
  (target) => {
    if (target) dialogOwner.value = target.owner;
  },
  { flush: 'sync' },
);
function retire() {
  repick.value = undefined;
  directory.retire();
  layout.retire();
}
watch(
  scope,
  (owner) => {
    retire();
    if (owner) void layout.refresh(owner);
  },
  { immediate: true, flush: 'sync' },
);
watch(
  () => downloads.value?.directory?.revision,
  (revision, previous) => {
    if (
      downloads.value &&
      (revision || previous) &&
      revision !== layoutValue.value?.revision &&
      !layoutFeedback.value?.recheck
    )
      void layout.refresh();
  },
);
onBeforeUnmount(() => {
  alive = false;
  retire();
});

const status = computed(() => {
  const value = migration.value;
  if (!value) return undefined;
  const done = value.items.filter((item) => item.phase !== 'pending').length;
  const error = downloads.value?.jobs.find((job) => job.id === value.id)?.error ?? value.error;
  if (error) return `下载目录迁移尚未完成（${done} / ${value.items.length} 项）：${error}`;
  if (value.phase === 'quiescing') return '正在等待下载任务暂停。';
  if (value.phase === 'rolling_back') return '正在撤销下载目录迁移。';
  return `正在迁移下载目录：${done} / ${value.items.length} 项。`;
});
const returnFocus = () => opener.value ?? null;
const fallbackFocus = () => migrationStatus.value ?? manage.value ?? null;
const mayRestoreFocus = computed(
  () => !!dialogOwner.value && directory.live(dialogOwner.value) && !repick.value,
);

function choose(event: MouseEvent) {
  if (!repick.value) void focus.run(event, () => directory.choose());
}
function confirm() {
  const shown = confirmation.value;
  if (!shown) return;
  if (shown.preview.decision === 'ready') void directory.confirm();
  else {
    repick.value = shown.owner;
    directory.dismiss();
  }
}
async function left() {
  const owner = repick.value;
  if (!owner) return;
  repick.value = undefined;
  if (!directory.live(owner) || document.querySelector('dialog[open]')) return;
  await directory.choose(owner);
  await nextTick();
  if (
    directory.live(owner) &&
    !document.querySelector('dialog[open]') &&
    document.activeElement === document.body &&
    opener.value?.isConnected &&
    !opener.value.disabled
  )
    opener.value.focus({ preventScroll: true });
}
function check(event: MouseEvent) {
  const owner = scope.value;
  if (!owner || activity.value || props.model.loading.value) return;
  void focus.run(event, () => directory.recheck(owner));
}
</script>

<template>
  <div class="settings-section-content">
    <p class="settings-note">下载目录（默认将音乐文件下载在该文件夹中）</p>
    <div class="settings-download-directory">
      <p class="settings-download-path" data-download-directory-path>
        {{
          account.status !== 'authenticated'
            ? '登录后可设置下载目录'
            : displayedRoot || (downloads ? '未设置' : readError ? '暂时无法读取' : '正在读取…')
        }}
      </p>
      <button
        ref="opener"
        type="button"
        class="secondary"
        :disabled="blocked() || !!activity || !!repick || !!feedback?.recheck"
        @click="choose"
      >
        更改目录
      </button>
    </div>
    <p
      v-if="downloads && (readError || feedback?.recheck)"
      class="settings-note"
      data-download-directory-stale
    >
      以上为上次读取的设置，当前目录尚未核对。
    </p>
    <p v-if="readError" class="settings-note" role="alert" data-download-directory-read-error>
      无法读取当前下载目录：{{ readError }}
    </p>
    <p
      v-if="activity === 'picking' || activity === 'previewing'"
      class="settings-note"
      role="status"
    >
      {{ activity === 'picking' ? '正在选择下载目录…' : '正在检查下载目录…' }}
    </p>
    <p
      v-if="status"
      ref="migrationStatus"
      class="settings-note"
      role="status"
      tabindex="-1"
      data-download-directory-migration
    >
      {{ status }}
    </p>
    <p v-if="feedback" class="settings-note" role="status" data-download-directory-feedback>
      {{ feedback.text }}
    </p>
    <div class="settings-download-actions">
      <button
        v-if="scope && (feedback?.recheck || readError)"
        type="button"
        class="secondary"
        :disabled="model.loading.value || !!activity"
        @click="check"
      >
        核对当前目录
      </button>
      <button type="button" class="secondary" @click="emit('preferences')">音质与下载选项</button>
      <button ref="manage" type="button" class="secondary" @click="emit('downloads')">
        打开下载管理
      </button>
    </div>
    <div ref="layoutControls" class="settings-download-layout">
      <fieldset
        v-for="group in layoutGroups"
        :key="group.field"
        class="settings-options"
        :data-download-layout-field="group.field"
        :disabled="!layoutValue"
        @keydown="guardLayoutKeys"
      >
        <legend>{{ group.title }}</legend>
        <div class="settings-choices">
          <label v-for="option in group.options" :key="option.value">
            <input
              type="radio"
              :name="`download-layout-${group.field}`"
              :value="option.value"
              :checked="layout.shown(group.field) === option.value"
              :aria-disabled="layoutDisabled || undefined"
              data-download-layout-option
              @click="layoutDisabled && $event.preventDefault()"
              @change="changeLayout($event, group.field, option.value)"
            />
            {{ option.label }}
          </label>
        </div>
      </fieldset>
      <p v-if="layoutActivity === 'reading' && !layoutValue" class="settings-note" role="status">
        正在读取下载命名与分类…
      </p>
      <p v-if="layoutFeedback" class="settings-note" role="status" data-download-layout-feedback>
        {{ layoutValue && layoutFeedback.recheck ? '以上为上次读取的设置。' : ''
        }}{{ layoutFeedback.text }}
      </p>
      <button
        v-if="layoutFeedback?.recheck"
        type="button"
        class="secondary"
        :disabled="!!layoutActivity"
        @click="checkLayout"
      >
        重新读取下载命名与分类
      </button>
    </div>
    <ModalTransition
      :return-focus="returnFocus"
      :fallback-focus="fallbackFocus"
      :restore-focus="mayRestoreFocus"
      @left="left"
    >
      <SettingsConfirm
        v-if="confirmation"
        title="更改下载目录"
        :description="directorySettingsDescription(confirmation.preview)"
        :confirm-label="confirmation.preview.decision === 'ready' ? '继续更改' : '重新选择'"
        :busy="activity === 'saving'"
        :error="''"
        @close="directory.dismiss"
        @confirm="confirm"
      />
    </ModalTransition>
  </div>
</template>
