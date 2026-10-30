<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue';
import { useCore } from '../../services/core';
import Icon from '../Icon.vue';
const props = defineProps<{
  lines: string[];
  truncated?: boolean;
  expanded: boolean;
  contentId: string;
}>();
const emit = defineEmits<{ toggle: [] }>();
const client = useCore();
const visible = computed(() => (props.expanded ? props.lines : props.lines.slice(0, 3)).join('\n'));
const copying = ref(false),
  message = ref(''),
  failed = ref(false);
let alive = true;
let timer: ReturnType<typeof setTimeout> | undefined;
onBeforeUnmount(() => {
  alive = false;
  clearTimeout(timer);
});
async function copy() {
  if (copying.value || !props.lines.length) return;
  copying.value = true;
  message.value = '';
  failed.value = false;
  clearTimeout(timer);
  try {
    await client.copyText(props.lines.join('\n'));
    if (alive) message.value = props.truncated ? '已复制显示部分' : '复制成功';
  } catch {
    if (alive) {
      failed.value = true;
      message.value = '复制失败，请重试';
    }
  } finally {
    if (alive) {
      copying.value = false;
      if (!failed.value)
        timer = setTimeout(() => {
          message.value = '';
        }, 3000);
    }
  }
}
</script>
<template>
  <div class="search-lyric-extra" :class="{ expanded }" @dblclick.stop>
    <div class="search-lyric-text">
      <Icon name="list" />
      <div>
        <p :id="contentId" :key="String(expanded)" class="search-lyric-lines">
          {{ lines.length ? visible : '暂无匹配歌词' }}
        </p>
        <p v-if="truncated" class="search-lyric-warning">歌词过长，仅显示部分内容</p>
      </div>
    </div>
    <div class="search-lyric-controls">
      <div class="search-lyric-buttons">
        <button
          class="secondary"
          data-result-action="expand"
          :aria-expanded="expanded"
          :aria-controls="contentId"
          :disabled="lines.length <= 3"
          @click="emit('toggle')"
        >
          <Icon name="down" />{{ expanded ? '收起歌词' : '展开歌词' }}
        </button>
        <button
          class="secondary"
          data-result-action="copy"
          :aria-disabled="copying || !lines.length"
          :disabled="!lines.length"
          @click="copy"
        >
          <Icon name="copy" />{{ copying ? '正在复制…' : truncated ? '复制显示部分' : '复制歌词' }}
        </button>
      </div>
      <span class="search-lyric-copy-status" :class="{ failed }" role="status" aria-live="polite">{{
        message
      }}</span>
    </div>
  </div>
</template>
