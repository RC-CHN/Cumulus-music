<script setup lang="ts">
import { computed, nextTick, ref } from 'vue';
const props = defineProps<{
  modelValue: string[];
  tags: { name: string; category: string }[];
  error: string;
  disabled: boolean;
}>();
const emit = defineEmits<{ 'update:modelValue': [value: string[]]; retry: [] }>();
const open = ref(false),
  trigger = ref<HTMLButtonElement>();
const categories = computed(() => [...new Set(props.tags.map((tag) => tag.category))]);
function toggle(tag: string) {
  if (props.disabled) return;
  emit(
    'update:modelValue',
    props.modelValue.includes(tag)
      ? props.modelValue.filter((t) => t !== tag)
      : [...props.modelValue, tag].slice(0, 3),
  );
}
async function close() {
  open.value = false;
  await nextTick();
  trigger.value?.focus();
}
</script>
<template>
  <div class="playlist-editor-tags" @keydown.esc.stop.prevent="open && close()">
    <div class="tag-selection">
      <button
        v-for="tag in modelValue"
        :key="tag"
        type="button"
        :disabled="disabled"
        :aria-label="`移除标签 ${tag}`"
        @click="toggle(tag)"
      >
        {{ tag }} ×
      </button>
      <button
        ref="trigger"
        type="button"
        :disabled="disabled"
        :aria-expanded="open"
        aria-controls="playlist-editor-tag-choices"
        @click="open = !open"
      >
        选择标签
      </button>
      <small>最多 3 个</small>
    </div>
    <div
      v-if="open"
      id="playlist-editor-tag-choices"
      class="tag-picker"
      @keydown.esc.stop.prevent="close"
    >
      <p v-if="error" role="alert">
        {{ error
        }}<button type="button" class="text-button" :disabled="disabled" @click="emit('retry')">
          重新加载标签
        </button>
      </p>
      <p v-else-if="!tags.length" role="status">正在加载标签…</p>
      <section v-for="category in categories" :key="category">
        <strong>{{ category }}</strong>
        <div>
          <button
            v-for="tag in tags.filter((t) => t.category === category)"
            :key="tag.name"
            type="button"
            :aria-pressed="modelValue.includes(tag.name)"
            :disabled="disabled || (modelValue.length >= 3 && !modelValue.includes(tag.name))"
            @click="toggle(tag.name)"
          >
            {{ tag.name }}
          </button>
        </div>
      </section>
      <button type="button" class="text-button" @click="close">完成选择</button>
    </div>
  </div>
</template>
