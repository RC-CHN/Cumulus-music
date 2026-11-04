<script setup lang="ts">
import ModerationReportDialog from '../ModerationReportDialog.vue';
import { useCore } from '../../services/core';
import {
  profileReportChoices,
  type ProfileReportChoice,
  type ReportCommand,
} from '../../services/profileReport';

defineProps<{
  epoch?: string;
  uid?: string;
  nickname?: string;
  choice: ProfileReportChoice | null;
  returnFocus: () => HTMLElement | null;
}>();
const emit = defineEmits<{ close: []; expired: [] }>();
const core = useCore();
function dispatch(epoch: string, uid: string, command: ReportCommand) {
  if (command.kind === 'prepare') {
    if (command.reason.kind === 'meaningless')
      return Promise.reject(new Error('reason unavailable'));
    return core.profileReport(epoch, uid, { kind: 'prepare', reason: command.reason });
  }
  return core.profileReport(epoch, uid, command);
}
</script>

<template>
  <ModerationReportDialog
    :epoch="epoch"
    :uid="uid"
    :choice="choice"
    :choices="profileReportChoices"
    :target-label="`举报用户：${nickname || '当前用户'}`"
    duplicate-message="你已经举报过该用户了哦"
    :return-focus="returnFocus"
    :dispatch="dispatch"
    @close="emit('close')"
    @expired="emit('expired')"
  />
</template>
