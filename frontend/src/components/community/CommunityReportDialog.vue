<script setup lang="ts">
import type { useCommunityReport } from '../../composables/useCommunityReport';
import { profileReportChoices } from '../../services/profileReport';
import ModerationReportDialog from '../ModerationReportDialog.vue';

const props = defineProps<{ model: ReturnType<typeof useCommunityReport> }>();
const emit = defineEmits<{ expired: [] }>();
const { target } = props.model;
let opener: HTMLElement | undefined;
function returnFocus() {
  if (target.value?.opener) opener = target.value.opener;
  return opener?.isConnected ? opener : null;
}
</script>

<template>
  <ModerationReportDialog
    :epoch="target?.epoch"
    :uid="target?.key"
    scope="community-event"
    :choice="target?.choice ?? null"
    :choices="profileReportChoices"
    :target-label="target ? `举报动态：${target.author}` : '举报动态'"
    duplicate-message="重复举报"
    :return-focus="returnFocus"
    :dispatch="model.dispatch"
    @close="model.close"
    @expired="emit('expired')"
  />
</template>
