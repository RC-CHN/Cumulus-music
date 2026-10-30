<script setup lang="ts">
import type { SearchWord } from '../../services/search';
defineProps<{
  query: string;
  title: string;
  recommended: SearchWord[];
  suggestions: SearchWord[];
  pending: boolean;
  composing: boolean;
  error?: string;
}>();
const emit = defineEmits<{ select: [keyword: string]; retry: [] }>();
function parts(label: string, query: string) {
  const at = label.toLocaleLowerCase().indexOf(query.toLocaleLowerCase());
  return at < 0
    ? [label, '', '']
    : [label.slice(0, at), label.slice(at, at + query.length), label.slice(at + query.length)];
}
</script>
<template>
  <button
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
        { title, words: recommended },
        { title: '搜索建议', words: suggestions },
      ]"
      :key="group.title"
    >
      <h3 v-if="group.words.length" class="search-section-heading">{{ group.title }}</h3>
      <button
        v-for="(word, i) in group.words"
        :key="i"
        data-search-choice
        class="search-suggestion"
        :title="word.label"
        @click="emit('select', word.keyword)"
      >
        <span
          >{{ parts(word.label, query)[0] }}<mark>{{ parts(word.label, query)[1] }}</mark
          >{{ parts(word.label, query)[2] }}</span
        ><small v-if="word.tag" class="search-word-tag">{{ word.tag }}</small>
      </button>
    </section>
    <p v-if="!recommended.length && !suggestions.length" class="search-muted">
      暂无建议，可直接搜索当前关键词
    </p>
  </template>
</template>
