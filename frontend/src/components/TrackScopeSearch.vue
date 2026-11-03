<script setup lang="ts">
import { ref } from 'vue';
import Icon from './Icon.vue';

defineProps<{ label: string; clearLabel: string }>();

const keyword = defineModel<string>({ required: true });
const field = ref<HTMLInputElement>();

function clear() {
  keyword.value = '';
  field.value?.focus({ preventScroll: true });
}

function escape(event: KeyboardEvent) {
  if (event.isComposing) return;
  event.preventDefault();
  event.stopPropagation();
  clear();
}
</script>

<template>
  <div class="track-scope-search" :class="{ 'has-keyword': keyword }">
    <Icon name="search" aria-hidden="true" />
    <input
      ref="field"
      v-model="keyword"
      type="search"
      :aria-label="label"
      placeholder="搜索"
      maxlength="128"
      @keydown.esc="escape"
    />
    <button v-if="keyword" type="button" :aria-label="clearLabel" @click="clear">
      <Icon name="close" aria-hidden="true" />
    </button>
  </div>
</template>
