<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue';
import type { SearchSuggestionWord } from '../../services/search';
import SearchMatchText from './SearchMatchText.vue';
const props = defineProps<{
  query: string;
  title: string;
  recommended: SearchSuggestionWord[];
  suggestions: SearchSuggestionWord[];
  pending: boolean;
  composing: boolean;
  error?: string;
}>();
const emit = defineEmits<{
  select: [keyword: string, display?: string, channel?: 'rec_query'];
  retry: [];
  restoreInputFocus: [];
}>();
const exactButton = ref<HTMLButtonElement>();
const pointer = shallowRef<HTMLButtonElement>();
function clearPointer() {
  pointer.value = undefined;
}
function point(event: PointerEvent) {
  if (
    document.activeElement instanceof HTMLButtonElement &&
    document.activeElement.hasAttribute('data-search-suggestion')
  )
    return;
  if (event.pointerType === 'mouse' || event.pointerType === 'pen')
    pointer.value = event.currentTarget as HTMLButtonElement;
}
function activePointer() {
  const button = pointer.value;
  return button?.isConnected && !button.disabled && !button.closest('[inert]') ? button : undefined;
}
watch(
  () => [
    props.query,
    props.pending,
    props.composing,
    props.suggestions,
    props.recommended,
    props.error,
  ],
  clearPointer,
  { flush: 'sync' },
);
defineExpose({ activePointer, clearPointer });
const showExact = computed(
  () => props.composing || props.pending || !!props.error || !props.suggestions.length,
);
// Check focus before Vue removes the temporary action.
watch(showExact, (show) => {
  if (!show && document.activeElement === exactButton.value) emit('restoreInputFocus');
});
</script>
<template>
  <div class="search-suggestions" @focusin="clearPointer" @pointerleave="clearPointer">
    <button
      v-if="showExact"
      ref="exactButton"
      class="search-suggestion search-exact"
      data-search-choice
      :title="`搜索“${query}”`"
      :disabled="composing"
      @click="emit('select', query)"
    >
      搜索“{{ query }}”
    </button>
    <p v-if="composing" class="search-muted" role="status">输入完成后显示搜索建议</p>
    <p v-else-if="pending" class="search-muted" role="status">正在查找搜索建议…</p>
    <div v-else-if="error" class="search-section-error" role="alert">
      <span>{{ error }}</span
      ><button @click="emit('retry')">重试搜索建议</button>
    </div>
    <template v-else>
      <section
        v-for="group in [
          { id: 'suggestions', title: '搜索建议', words: suggestions },
          { id: 'recommendations', title, words: recommended },
        ]"
        :key="group.id"
      >
        <h3 v-if="group.words.length" class="search-section-heading">{{ group.title }}</h3>
        <button
          v-for="(word, i) in group.words"
          :key="i"
          data-search-choice
          data-search-suggestion
          :data-suggestion-key="`${group.id}:${i}`"
          class="search-suggestion"
          :class="{
            'search-suggestion-pointed': pointer?.dataset.suggestionKey === `${group.id}:${i}`,
          }"
          :title="word.label"
          @pointerenter="point"
          @click="
            emit(
              'select',
              word.keyword,
              word.label,
              group.id === 'recommendations' ? 'rec_query' : undefined,
            )
          "
        >
          <span><SearchMatchText :text="word.label" :parts="word.labelParts" /></span
          ><small v-if="word.tag" class="search-word-tag">{{ word.tag }}</small>
        </button>
      </section>
      <p v-if="!recommended.length && !suggestions.length" class="search-muted">
        暂无建议，可直接搜索当前关键词
      </p>
    </template>
  </div>
</template>
