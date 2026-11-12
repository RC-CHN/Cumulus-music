<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import {
  useDefaultQualitySettings,
  type QualitySettingsVisit,
} from '../../composables/useDefaultQualitySettings';
import type { DownloadModel } from '../../composables/useDownloads';
import type { PlayerModel } from '../../composables/usePlayer';
import { failure, type AccountView } from '../../services/contracts';
import { useCore } from '../../services/core';
import DefaultQualityChoices from './DefaultQualityChoices.vue';
import Icon from '../Icon.vue';

const props = defineProps<{
  account: AccountView;
  player: PlayerModel;
  downloads: DownloadModel;
  visit: () => object | undefined;
}>();
const emit = defineEmits<{ expired: []; login: []; member: [] }>();
const core = useCore();
const signedIn = computed(() => props.account.status === 'authenticated');
const owner = computed<QualitySettingsVisit | undefined>((previous) => {
  const page = props.visit();
  if (!page || (!signedIn.value && props.account.status !== 'signed_out')) return;
  const epoch = props.account.epoch;
  return previous?.page === page && previous.epoch === epoch ? previous : { page, epoch };
});
const downloadOwner = computed(() => (signedIn.value ? owner.value : undefined));
const playback = useDefaultQualitySettings({
  visit: () => owner.value,
  read: (epoch, refresh) => core.playbackQualityPreferences(epoch, refresh),
  save: (epoch, revision, quality) => core.savePlaybackQualityPreference(epoch, revision, quality),
  expired: () => emit('expired'),
});
const download = useDefaultQualitySettings({
  visit: () => downloadOwner.value,
  async read(epoch, refresh) {
    const settings = await core.downloadQualityPreferences(epoch, refresh);
    return {
      epoch: settings.epoch,
      revision: settings.revision,
      quality: settings.options.quality,
      choices: settings.choices,
    };
  },
  async save(epoch, revision, quality) {
    const result = await core.downloadCommand(epoch, {
      kind: 'default_quality',
      revision,
      quality,
    });
    if (result.epoch !== epoch || result.failed.length)
      throw { code: 'stale', message: '未能核对当前下载音质。' };
  },
  expired: () => emit('expired'),
  changed: () => void props.downloads.refresh(true),
});
const guideBusy = ref(false),
  guideError = ref('');
async function refreshQuality(kind: 'playback' | 'download') {
  const visit = owner.value;
  if (!visit) return;
  const [selected, sibling] = kind === 'playback' ? [playback, download] : [download, playback];
  if ((await selected.refresh(true)) && owner.value === visit && signedIn.value)
    await sibling.refresh();
}
watch(
  owner,
  (visit) => {
    playback.retire();
    guideBusy.value = false;
    guideError.value = '';
    if (visit) void playback.refresh();
  },
  { immediate: true, flush: 'sync' },
);
watch(
  downloadOwner,
  (visit) => {
    download.retire();
    if (visit) void download.refresh();
  },
  { immediate: true, flush: 'sync' },
);
watch(
  () => props.downloads.view.value?.options.quality,
  (quality) => {
    if (
      quality &&
      download.value.value &&
      quality !== download.value.value.quality &&
      !download.readbackNeeded.value
    )
      void download.refresh();
  },
);
watch(
  () => props.player.view.value?.qualityPreferences.revision,
  () => {
    if (!playback.readbackNeeded.value) void playback.refresh();
  },
);
let alive = true;
onBeforeUnmount(() => {
  alive = false;
  playback.retire();
  download.retire();
});
async function openGuide() {
  const visit = owner.value;
  if (!visit || guideBusy.value) return;
  guideBusy.value = true;
  guideError.value = '';
  try {
    await core.openSettingsQualityGuide(visit.epoch);
  } catch (cause) {
    if (alive && owner.value === visit) guideError.value = failure(cause).message;
  } finally {
    if (alive && owner.value === visit) guideBusy.value = false;
  }
}
</script>

<template>
  <div class="settings-quality">
    <DefaultQualityChoices
      kind="playback"
      :value="playback.value.value"
      :activity="playback.activity.value"
      :feedback="playback.feedback.value"
      :needs-readback="playback.readbackNeeded.value"
      :signed-in="signedIn"
      :available="!!owner"
      @choose="playback.choose"
      @refresh="refreshQuality('playback')"
      @login="emit('login')"
      @member="emit('member')"
    />
    <DefaultQualityChoices
      kind="download"
      :value="download.value.value"
      :activity="download.activity.value"
      :feedback="download.feedback.value"
      :needs-readback="download.readbackNeeded.value"
      :signed-in="signedIn"
      :available="!!downloadOwner"
      @choose="download.choose"
      @refresh="refreshQuality('download')"
      @login="emit('login')"
      @member="emit('member')"
    />
    <button
      v-if="owner"
      class="text-button settings-quality-guide"
      type="button"
      :aria-disabled="guideBusy || undefined"
      @click="openGuide"
    >
      了解音质 <Icon name="share" />
    </button>
    <p v-if="guideError" class="settings-note" role="status">{{ guideError }}</p>
  </div>
</template>
