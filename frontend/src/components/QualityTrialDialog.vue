<script setup lang="ts">
import { computed, nextTick, onMounted, ref, useId, watchEffect } from 'vue';
import type { PlaybackQualityModel, QualityPrompt } from '../composables/usePlaybackQuality';
import { qualityDuration, qualityLabel } from '../services/playbackQuality';
import Icon from './Icon.vue';
const props = defineProps<{ model: PlaybackQualityModel; target: QualityPrompt }>();
const emit = defineEmits<{ close: []; refresh: []; member: [] }>();
const prompt = computed(() => props.target);
const trial = computed(() => prompt.value.trial);
const busy = ref(props.model.dialogBusy.value),
  error = ref(props.model.dialogError.value),
  changed = ref(props.model.targetChanged.value);
// Retain the outgoing surface's dimensions/text until its leave animation ends.
watchEffect(() => {
  if (props.model.prompt.value) {
    busy.value = props.model.dialogBusy.value;
    error.value = props.model.dialogError.value;
    changed.value = props.model.targetChanged.value;
  }
});
const name = computed(() => qualityLabel(prompt.value.quality));
const title = computed(() =>
  prompt.value.kind === 'notice'
    ? '音质使用提示'
    : prompt.value.kind === 'confirm'
      ? `试用${name.value}`
      : trial.value?.canUse
        ? '音质已启用'
        : '核对音质试用',
);
const duration = computed(() => qualityDuration(prompt.value.choice?.remainingMs));
const member = computed(() =>
  ['requires_vip', 'requires_svip'].includes(prompt.value.choice?.access ?? ''),
);
const pending = computed(
  () =>
    trial.value && ['sending', 'uncertain', 'acknowledged', 'active'].includes(trial.value.phase),
);
const dialog = ref<HTMLDialogElement>(),
  cancel = ref<HTMLButtonElement>();
const id = useId();
onMounted(() => {
  dialog.value?.showModal();
  void nextTick(() => cancel.value?.focus({ preventScroll: true }));
});
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog quality-trial-dialog"
    :aria-labelledby="`${id}-title`"
    :aria-describedby="`${id}-description`"
    :data-kind="prompt.kind"
    :data-phase="trial?.phase ?? 'offer'"
    :aria-busy="busy"
    @cancel.prevent="emit('close')"
  >
    <header>
      <h2 :id="`${id}-title`">{{ title }}</h2>
      <button class="icon-button" aria-label="关闭音质对话框" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
    <div class="quality-trial-body" :id="`${id}-description`">
      <template v-if="prompt.kind === 'confirm'">
        <p class="quality-trial-offer">
          {{ duration ? `可试用 ${duration}` : '当前账号可试用此音质' }}
        </p>
        <p>确认后开始计时，暂停或关闭客户端不会停止计时。到期后恢复之前的音质偏好。</p>
      </template>
      <p v-else-if="prompt.kind === 'notice'">{{ prompt.message }}</p>
      <template v-else>
        <p v-if="trial?.canUse">
          已确认可使用{{ name }}。当前歌曲尚未切换，可使用已有权益继续切换音质。
        </p>
        <p v-else-if="trial?.phase === 'expired'">
          这次音质试用已结束，请重新打开菜单选择其他音质。
        </p>
        <p v-else-if="trial?.phase === 'rejected'">
          这次启用未通过服务校验，请刷新音质后重新选择。
        </p>
        <p v-else-if="trial?.readFailed">暂时无法确认试用权益，请核对结果后继续。</p>
        <p v-else>尚未确认这次启用的结果，试用可能已经开始。请核对结果，避免重复启用。</p>
        <p v-if="trial?.acknowledged && !trial.canUse" class="muted">
          已收到启用回执，仍需确认当前可使用的权益。
        </p>
        <p v-if="changed" class="quality-warning">
          歌曲或音质偏好已变化，请在当前歌曲的菜单重新选择。
        </p>
      </template>
      <p v-if="busy" role="status" class="muted">
        正在处理，关闭后仍会继续；可稍后从音质菜单核对结果。
      </p>
      <p v-if="error" role="alert" class="quality-warning">{{ error }}</p>
    </div>
    <footer>
      <button ref="cancel" @click="emit('close')">
        {{ busy ? '稍后查看' : prompt.kind === 'confirm' ? '取消' : '关闭' }}
      </button>
      <button
        v-if="prompt.kind === 'confirm'"
        class="primary"
        :aria-disabled="busy || changed"
        @click="model.start()"
      >
        {{ busy ? '正在启用…' : '确认启用试用' }}
      </button>
      <button
        v-else-if="prompt.kind === 'notice' && member"
        class="primary"
        @click="emit('member')"
      >
        查看会员权益
      </button>
      <button
        v-else-if="trial?.canUse && !changed"
        class="primary"
        :aria-disabled="busy"
        @click="model.useActive()"
      >
        {{ busy ? '正在切换…' : '使用已启用音质' }}
      </button>
      <button v-else-if="pending" class="primary" :aria-disabled="busy" @click="model.reconcile()">
        {{ busy ? '正在核对…' : '核对试用结果' }}
      </button>
      <button v-else class="primary" :aria-disabled="busy" @click="!busy && emit('refresh')">
        刷新音质
      </button>
    </footer>
  </dialog>
</template>
