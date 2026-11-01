<script setup lang="ts">
import { computed, ref, nextTick } from 'vue';
import type { CloudDeleteTarget } from '../../services/cloudDelete';
const props = defineProps<{ target: CloudDeleteTarget; disabled: boolean; submitted: boolean }>();
const offset = ref(0),
  root = ref<HTMLElement>(),
  list = ref<HTMLElement>();
const count = 20;
const titles = computed(() => props.target.titles.slice(offset.value, offset.value + count));
async function move(direction: number) {
  if (props.disabled) return;
  const origin = document.activeElement;
  offset.value = Math.max(
    0,
    Math.min(
      Math.floor((props.target.titles.length - 1) / count) * count,
      offset.value + direction * count,
    ),
  );
  await nextTick();
  list.value?.scrollTo({ top: 0 });
  if (origin instanceof HTMLElement && origin.matches(':disabled'))
    root.value?.querySelector<HTMLElement>('button:not(:disabled)')?.focus();
}
</script>
<template>
  <section ref="root" class="cloud-delete-review" aria-label="待删除歌曲范围" tabindex="-1">
    <p v-if="target.ids.length > count">
      <template v-if="submitted"
        >本次查看 {{ target.ids.length }} 首，以下为打开确认时固定的范围。</template
      >
      <template v-else>共 {{ target.ids.length }} 首，确认后将处理全部所选歌曲。</template>
    </p>
    <ul ref="list" class="cloud-delete-targets" aria-label="待删除歌曲" tabindex="0">
      <li v-for="(title, index) in titles" :key="offset + index">{{ title }}</li>
    </ul>
    <nav
      v-if="target.titles.length > count"
      class="cloud-delete-pagination"
      aria-label="确认列表分页"
    >
      <button class="text-button" :disabled="disabled || !offset" @click="move(-1)">上一页</button>
      <span
        >{{ offset + 1 }}–{{ Math.min(offset + count, target.titles.length) }} /
        {{ target.titles.length }}</span
      >
      <button
        class="text-button"
        :disabled="disabled || offset + count >= target.titles.length"
        @click="move(1)"
      >
        下一页
      </button>
    </nav>
  </section>
</template>
