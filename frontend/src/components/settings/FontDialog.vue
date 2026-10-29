<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { createFontClient } from '../../adapters/fonts';
import { useFonts } from '../../composables/useFonts';
import { useControlFocus } from '../../composables/useControlFocus';
import { fontStack } from '../../services/fonts';
import Icon from '../Icon.vue';
const props = defineProps<{ epoch: string; value: string }>();
const emit = defineEmits<{ close: []; select: [name: string] }>();
const model = useFonts(createFontClient(), props.epoch);
const dialog = ref<HTMLDialogElement>(),
  search = ref<HTMLInputElement>(),
  list = ref<HTMLElement>();
const selected = ref(props.value);
const focus = useControlFocus(
  () => props.epoch,
  () => search.value,
);
const preview = computed(() =>
  fontStack([selected.value, ...(model.catalog.value?.fallbacks ?? [])]),
);
watch(model.query, () => {
  model.page.value = 0;
});
watch([model.query, model.page], async () => {
  await nextTick();
  if (list.value) list.value.scrollTop = 0;
});
onMounted(() => {
  dialog.value?.showModal();
  search.value?.focus();
  void model.read();
});
onUnmounted(() => {
  model.dispose();
});
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog font-dialog"
    aria-labelledby="font-dialog-title"
    @cancel.prevent="emit('close')"
  >
    <header>
      <h2 id="font-dialog-title">选择本机字体</h2>
      <button class="icon-button" aria-label="关闭字体选择" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
    <div class="font-search">
      <input
        ref="search"
        v-model="model.query.value"
        type="search"
        aria-label="搜索本机字体"
        placeholder="搜索字体名称"
      /><button
        class="text-button"
        :disabled="model.loading.value"
        @click="focus.run($event, model.read)"
      >
        刷新
      </button>
    </div>
    <p v-if="model.loading.value" class="settings-note" role="status">正在读取本机字体…</p>
    <p v-if="model.error.value" class="catalog-error" role="alert">{{ model.error.value }}</p>
    <div class="font-picker-body">
      <div class="font-options">
        <label class="font-option font-default"
          ><input
            v-model="selected"
            type="radio"
            name="lyric-font"
            value=""
          />默认（自动选择）</label
        >
        <div ref="list" class="font-list" aria-label="可用字体" :aria-busy="model.loading.value">
          <label
            v-for="family in model.rows.value"
            :key="family.name"
            class="font-option"
            :title="family.name"
            ><input
              v-model="selected"
              type="radio"
              name="lyric-font"
              :value="family.name"
            /><span>{{ family.name }}</span></label
          >
          <p
            v-if="model.catalog.value && !model.filtered.value.length"
            class="settings-note"
            role="status"
          >
            没有找到匹配的字体。
          </p>
        </div>
        <nav class="font-pagination" aria-label="字体分页">
          <button
            class="icon-button"
            aria-label="上一页字体"
            :disabled="!model.page.value"
            @click="focus.run($event, () => model.page.value--)"
          >
            <Icon name="back" /></button
          ><span>{{ model.page.value + 1 }} / {{ model.pages.value }}</span
          ><button
            class="icon-button"
            aria-label="下一页字体"
            :disabled="model.page.value + 1 >= model.pages.value"
            @click="focus.run($event, () => model.page.value++)"
          >
            <Icon name="forward" /></button
          ><small>{{ model.filtered.value.length }} 种字体</small>
        </nav>
      </div>
      <section class="font-preview" aria-label="字体预览">
        <span class="settings-note">预览</span><strong>{{ selected || '默认字体' }}</strong>
        <p :style="{ fontFamily: preview }">让音乐陪伴你<br />Cumulus 012345</p>
        <p
          v-if="selected && model.catalog.value && !model.installed(selected)"
          class="font-fallback-note"
          role="status"
        >
          本机未找到此字体，将自动使用备用字体。已保存的名称会保留。
        </p>
        <p class="settings-note">选择后返回设置，点击“应用样式”保存。</p>
      </section>
    </div>
    <footer>
      <button class="secondary" @click="emit('close')">取消</button
      ><button class="primary" @click="emit('select', selected)">使用此字体</button>
    </footer>
  </dialog>
</template>
<style scoped>
.font-dialog {
  width: min(720px, calc(100vw - 40px));
  max-height: calc(100dvh - 48px);
  overflow-y: auto;
}
.font-dialog h2 {
  font-size: 20px;
}
.font-dialog header {
  margin-bottom: 0;
}
.font-search {
  display: flex;
  align-items: center;
  gap: 12px;
  margin: 18px 0;
}
.font-search input {
  flex: 1;
  min-width: 0;
  background: var(--surface);
  color: var(--text);
  border: 1px solid var(--line-strong);
  border-radius: 8px;
  padding: 10px 12px;
  font: inherit;
}
.font-picker-body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 24px;
}
.font-options {
  min-width: 0;
}
.font-list {
  height: 240px;
  overflow-y: auto;
  overscroll-behavior: contain;
  border-bottom: 1px solid var(--line);
}
.font-option {
  display: flex;
  flex-direction: row;
  margin: 0;
  align-items: center;
  gap: 10px;
  min-height: 36px;
  padding: 6px 10px;
  border-radius: 6px;
  cursor: pointer;
}
.font-option:hover {
  background: var(--surface-hover);
}
.font-option:has(input:checked) {
  background: var(--accent-soft);
}
.font-dialog .font-option input[type='radio'] {
  width: 14px;
  height: 14px;
  padding: 0;
  margin: 0;
  flex-shrink: 0;
  accent-color: var(--accent);
}
.font-option span {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.font-default {
  margin-bottom: 8px;
}
.font-pagination {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
  font-variant-numeric: tabular-nums;
}
.font-pagination small {
  margin-left: auto;
  color: var(--muted);
}
.font-preview {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
  padding: 18px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--surface);
  overflow-wrap: anywhere;
}
.font-preview strong {
  font-size: 14px;
  font-weight: 500;
}
.font-preview > p {
  margin: 0;
}
.font-preview > p[style] {
  font-size: 28px;
  line-height: 1.65;
  margin: 12px 0 auto;
}
.font-fallback-note {
  font-size: 12px;
  color: var(--muted);
  line-height: 1.6;
}
.font-dialog footer {
  position: static;
  padding: 0;
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  margin-top: 20px;
}
@media (max-height: 650px) {
  .font-search {
    margin: 12px 0;
  }
  .font-preview {
    padding: 14px;
    gap: 8px;
  }
  .font-list {
    height: 180px;
  }
  .font-preview > p[style] {
    font-size: 22px;
    line-height: 1.5;
    margin: 4px 0 auto;
  }
  .font-dialog footer {
    margin-top: 14px;
  }
}
@media (max-width: 600px) {
  .font-picker-body {
    grid-template-columns: minmax(0, 1fr);
    gap: 14px;
  }
  .font-list {
    height: 160px;
  }
}
</style>
