<script setup lang="ts">
import { computed, nextTick, ref, onScopeDispose, watch } from 'vue';
import type { CloudPage } from '../../services/cloud';
import { cloudBytes } from '../../services/cloud';
import { cloudCapacityText, cloudCapacityPercent } from '../../services/cloudSongs';
import { useCore } from '../../services/core';
import { failure } from '../../services/contracts';
const props = defineProps<{ epoch: string; view?: CloudPage }>();
const emit = defineEmits<{ expired: [] }>();
const root = ref<HTMLElement>(),
  trigger = ref<HTMLButtonElement>(),
  opening = ref(false),
  feedback = ref(''),
  failed = ref(false);
const client = useCore();
let alive = true;
onScopeDispose(() => {
  alive = false;
});
watch(
  () => props.epoch,
  () => {
    opening.value = false;
    feedback.value = '';
    failed.value = false;
  },
  { flush: 'sync' },
);
const full = computed(
  () => !!props.view?.maxSize && props.view.size !== null && props.view.size >= props.view.maxSize,
);
const percent = computed(() =>
  cloudCapacityPercent(props.view?.size ?? null, props.view?.maxSize ?? null),
);
async function expand() {
  if (opening.value || !props.epoch) return;
  const owner = props.epoch;
  opening.value = true;
  feedback.value = '';
  root.value?.focus({ preventScroll: true });
  try {
    await client.openCloudCapacity(owner);
    if (alive && props.epoch === owner) {
      failed.value = false;
      feedback.value = '已请求在浏览器打开扩容页面';
    }
  } catch (error) {
    if (alive && props.epoch === owner) {
      const issue = failure(error);
      feedback.value = issue.message;
      failed.value = true;
      if (['session_expired', 'profile_changed'].includes(issue.code)) emit('expired');
    }
  } finally {
    if (alive && props.epoch === owner) {
      opening.value = false;
      await nextTick();
      if (document.activeElement === root.value) trigger.value?.focus({ preventScroll: true });
    }
  }
}
</script>
<template>
  <div ref="root" class="cloud-capacity" aria-label="网盘容量" tabindex="-1">
    <span>网盘容量</span
    ><progress
      :value="percent ?? 0"
      max="100"
      aria-label="云盘空间使用量"
      :aria-valuetext="percent == null ? '容量尚未读取' : undefined"
      :class="{ unknown: percent == null }"
    />
    <span
      class="cloud-capacity-numbers"
      :title="`${cloudBytes(view?.size ?? null)} / ${cloudBytes(view?.maxSize ?? null)}`"
      >{{ cloudCapacityText(view?.size) }}/{{ cloudCapacityText(view?.maxSize) }}</span
    >
    <span
      v-if="view?.revision && view.uploadedCount > 0 && percent != null"
      class="cloud-capacity-tip"
      :class="{ full }"
      >{{
        full ? '网盘已满，请清除部分文件，才能继续上传' : '歌曲永久保存，随时随地多端畅听'
      }}</span
    >
    <button
      ref="trigger"
      class="text-button cloud-capacity-expand"
      :disabled="!view?.revision || opening"
      :aria-busy="opening"
      @click="expand"
    >
      {{ opening ? '正在打开…' : '扩容' }}
    </button>
    <span
      v-if="feedback"
      class="cloud-capacity-feedback"
      :class="{ failed }"
      :role="failed ? 'alert' : 'status'"
      >{{ feedback
      }}<button v-if="failed" class="text-button" :disabled="opening" @click="expand">重试</button
      ><button class="text-button" @click="feedback = ''">关闭</button></span
    >
  </div>
</template>
