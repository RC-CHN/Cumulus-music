<script setup lang="ts">
import { computed, nextTick, ref, useId, watch } from 'vue';
import { useTagPopover } from '../../composables/useTagPopover';
const props = defineProps<{
  anchor: HTMLElement | undefined;
  selected: string[];
  tags: { name: string; category: string }[];
  loading: boolean;
  error: string;
  disabled: boolean;
  id: string;
}>();
const emit = defineEmits<{
  toggle: [tag: string];
  cancel: [];
  confirm: [];
  dismiss: [restore: boolean, direction: -1 | 0 | 1];
  retry: [];
}>();
const panel = ref<HTMLElement>();
const style = useTagPopover(
  panel,
  () => props.anchor,
  () => emit('dismiss', false, 0),
);
const categories = computed(() => [...new Set(props.tags.map((tag) => tag.category))]);
const category = ref('');
const prefix = useId();
const tabId = (index: number) => `${prefix}-tab-${index}`;
const paneId = `${prefix}-pane`;
watch(
  categories,
  (values) => {
    if (!values.includes(category.value)) category.value = values[0] ?? '';
  },
  { immediate: true },
);
const choices = computed(() => props.tags.filter((tag) => tag.category === category.value));
async function selectCategory(value: string) {
  category.value = value;
  await nextTick();
  panel.value?.querySelector<HTMLElement>('[aria-selected="true"]')?.focus({ preventScroll: true });
}
function tabsKey(event: KeyboardEvent) {
  const values = categories.value;
  if (!values.length || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  const current = values.indexOf(category.value);
  const next =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? values.length - 1
        : (current + (event.key === 'ArrowRight' ? 1 : -1) + values.length) % values.length;
  void selectCategory(values[next]!);
}
function key(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault();
    event.stopPropagation();
    emit('dismiss', true, 0);
  } else if (event.key === 'Tab') {
    const nodes = [
      ...(panel.value?.querySelectorAll<HTMLElement>(
        'button:not(:disabled):not([tabindex="-1"])',
      ) ?? []),
    ];
    if (
      (event.shiftKey && document.activeElement === nodes[0]) ||
      (!event.shiftKey && document.activeElement === nodes.at(-1))
    ) {
      event.preventDefault();
      emit('dismiss', true, event.shiftKey ? -1 : 1);
    }
  } else if (event.key === ' ' && event.target === panel.value) event.stopPropagation();
}
</script>
<template>
  <div
    ref="panel"
    :id="id"
    class="tag-picker"
    role="dialog"
    aria-label="选择歌单标签"
    tabindex="-1"
    :style="style"
    @keydown="key"
  >
    <div v-if="loading" class="tag-catalogue-state" role="status">
      <span class="spinner" />
      <p>正在加载标签…</p>
    </div>
    <div v-else-if="error" class="tag-catalogue-state" role="alert">
      <p>{{ error }}</p>
      <button type="button" class="text-button" :disabled="disabled" @click="emit('retry')">
        重新加载标签
      </button>
    </div>
    <div v-else-if="!tags.length" class="tag-catalogue-state" role="status">
      <p>暂时没有可选标签</p>
      <button type="button" class="text-button" :disabled="disabled" @click="emit('retry')">
        重新加载标签
      </button>
    </div>
    <template v-else>
      <div class="tag-category-tabs" role="tablist" aria-label="标签分类" @keydown="tabsKey">
        <button
          v-for="(value, index) in categories"
          :id="tabId(index)"
          :key="value"
          type="button"
          role="tab"
          :aria-selected="category === value"
          :aria-controls="paneId"
          :tabindex="category === value ? 0 : -1"
          :title="value"
          @click="selectCategory(value)"
        >
          {{ value }}
        </button>
      </div>
      <div class="tag-catalogue-scroll">
        <Transition name="tab" mode="out-in"
          ><section
            :id="paneId"
            :key="category"
            role="tabpanel"
            :aria-labelledby="tabId(categories.indexOf(category))"
            class="tag-category-pane"
          >
            <button
              v-for="tag in choices"
              :key="tag.name"
              type="button"
              :title="tag.name"
              :aria-pressed="selected.includes(tag.name)"
              :disabled="disabled || (selected.length >= 3 && !selected.includes(tag.name))"
              @click="emit('toggle', tag.name)"
            >
              {{ tag.name }}
            </button>
          </section></Transition
        >
      </div>
    </template>
    <p class="tag-limit" role="status">
      {{
        selected.length >= 3
          ? '已选择 3 个标签，移除一个后可继续选择'
          : `已选择 ${selected.length} / 3 个标签`
      }}
    </p>
    <footer>
      <button
        type="button"
        class="download-button"
        aria-label="取消标签选择"
        @click="emit('cancel')"
      >
        取消</button
      ><button type="button" class="primary" aria-label="完成选择" @click="emit('confirm')">
        确定
      </button>
    </footer>
  </div>
</template>
