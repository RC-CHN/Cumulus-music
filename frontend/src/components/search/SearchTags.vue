<script setup lang="ts">
import { ref, watch, nextTick, onUnmounted } from 'vue';
import type { SearchWord } from '../../services/search';
import Icon from '../Icon.vue';
const props = defineProps<{
  title: string;
  words: SearchWord[];
  history?: boolean;
  busy?: boolean;
  emptyLabel?: string;
}>();
const emit = defineEmits<{ select: [word: string]; remove: [word: string]; clear: [] }>();
const expanded = ref(false),
  foldable = ref(false),
  visibleCount = ref(0),
  row = ref<HTMLElement>(),
  fold = ref<HTMLButtonElement>();
function measure() {
  const list = row.value;
  if (!list?.clientWidth) return;
  // Hidden tags retain their intrinsic width so a later resize can reveal them
  // without a second copy of the controls or flashing all tags on screen.
  const tags = [...list.querySelectorAll<HTMLElement>('.search-tag')];
  const widths = tags.map((tag) => tag.getBoundingClientRect().width);
  const gap = parseFloat(getComputedStyle(list).columnGap);
  function fits(count: number, toggle: boolean) {
    let rows = 1,
      used = 0;
    for (const width of [...widths.slice(0, count), ...(toggle ? [24] : [])]) {
      if (used && used + gap + width > list!.clientWidth) {
        rows++;
        used = width;
      } else used += (used ? gap : 0) + width;
    }
    return rows <= 2;
  }
  foldable.value = !fits(tags.length, false);
  let count = tags.length;
  if (foldable.value) while (count > 0 && !fits(count, true)) count--;
  visibleCount.value = count;
  if (!expanded.value && tags.slice(count).some((tag) => tag.contains(document.activeElement)))
    void nextTick(() => fold.value?.focus({ preventScroll: true }));
}
async function toggle() {
  expanded.value = !expanded.value;
  await nextTick();
  fold.value?.scrollIntoView({ block: 'nearest' });
}
const observer = new ResizeObserver(measure);
watch(
  [() => props.words, row, expanded],
  async () => {
    await nextTick();
    observer.disconnect();
    if (row.value) {
      observer.observe(row.value);
      measure();
    }
  },
  { flush: 'post' },
);
onUnmounted(() => observer.disconnect());
</script>
<template>
  <section class="search-tags-section" :class="{ 'search-history-tags': history }">
    <header class="search-section-heading">
      <h3>{{ title }}</h3>
      <button
        v-if="history && words.length"
        class="icon-button"
        aria-label="清空搜索历史"
        :disabled="busy"
        @click="emit('clear')"
      >
        <Icon name="trash" />
      </button>
    </header>
    <p v-if="!words.length && emptyLabel" class="search-muted">{{ emptyLabel }}</p>
    <div v-if="words.length" ref="row" class="search-tags" :class="{ expanded }">
      <span
        v-for="(word, i) in words"
        :key="word.keyword + '-' + i"
        class="search-tag"
        :class="{ 'search-tag-hidden': !expanded && i >= visibleCount }"
        :inert="!expanded && i >= visibleCount"
        :aria-hidden="!expanded && i >= visibleCount"
      >
        <button data-search-choice :title="word.label" @click="emit('select', word.keyword)">
          {{ word.label }}
        </button>
        <button
          v-if="history"
          class="search-tag-remove"
          :aria-label="`删除搜索历史：${word.label}`"
          :disabled="busy"
          @click="emit('remove', word.keyword)"
        >
          <Icon name="close" />
        </button>
      </span>
      <button
        v-if="foldable"
        ref="fold"
        class="search-expand"
        :aria-label="`${expanded ? '收起' : '展开'}${title}`"
        :title="`${expanded ? '收起' : '展开'}${title}`"
        :aria-expanded="expanded"
        @click="toggle"
      >
        <Icon :name="expanded ? 'up' : 'down'" />
      </button>
    </div>
  </section>
</template>
