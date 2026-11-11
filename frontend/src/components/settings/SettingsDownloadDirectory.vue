<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import type { DownloadModel } from '../../composables/useDownloads';
import {
  createDownloadDirectorySettings,
  directorySettingsDescription,
  displayDownloadDirectory,
  type DownloadDirectorySettingsOwner,
} from '../../composables/useDownloadDirectorySettings';
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
  !!migration.value;
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
const focus = useControlFocus(
  () => scope.value,
  () => opener.value,
);
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
}
watch([() => scope.value?.epoch, () => scope.value?.visit], retire, { flush: 'sync' });
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
