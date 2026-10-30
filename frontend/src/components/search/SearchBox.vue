<script setup lang="ts">
import MenuTransition from '../MenuTransition.vue';
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import { useSearchBox } from '../../composables/useSearchBox';
import Icon from '../Icon.vue';
import ModalTransition from '../ModalTransition.vue';
import SearchTags from './SearchTags.vue';
import SearchCharts from './SearchCharts.vue';
import SearchSuggestions from './SearchSuggestions.vue';
import SearchHistoryConfirm from './SearchHistoryConfirm.vue';
const props = defineProps<{ account: AccountView; pageKey: string; keyword?: string }>();
const emit = defineEmits<{ search: [keyword: string]; expired: [] }>();
const model = useSearchBox(undefined, () => emit('expired'));
const {
  query,
  open,
  composing,
  defaultWord,
  history,
  recommendations,
  charts,
  suggestions,
  recommended,
  suggestionTitle,
  errors,
  pending,
  searching,
} = model;
const host = ref<HTMLElement>(),
  input = ref<HTMLInputElement>(),
  panel = ref<HTMLElement>();
const position = ref({ left: '0px', top: '0px', width: '779px', maxHeight: '622px' });
const confirm = ref(false);
let suppressFocus = false;
let frame = 0,
  compositionEnded = 0;
const historyWords = computed(() =>
  history.value.map((keyword) => ({ keyword, label: keyword, tag: null })),
);
function place() {
  const r = input.value?.closest('form')?.getBoundingClientRect();
  if (!r) return;
  const width = Math.min(779, innerWidth - 24),
    top = r.bottom + 8;
  position.value = {
    left: Math.min(Math.max(12, r.left - 36), innerWidth - width - 12) + 'px',
    top: top + 'px',
    width: width + 'px',
    maxHeight: Math.max(80, Math.min(622, innerHeight - top - 92)) + 'px',
  };
}
function resize() {
  cancelAnimationFrame(frame);
  frame = requestAnimationFrame(place);
}
async function show() {
  model.show();
  place();
  await nextTick();
  if (panel.value) panel.value.scrollTop = 0;
}
function focusInput() {
  suppressFocus = true;
  input.value?.focus({ preventScroll: true });
  suppressFocus = false;
}
function onFocus() {
  if (!suppressFocus) void show();
}
function submit(word?: string) {
  if (composing.value || performance.now() - compositionEnded < 60) return;
  const value = model.submit(word);
  if (value) {
    focusInput();
    emit('search', value);
  }
}
async function clear() {
  model.input('');
  await show();
  input.value?.focus();
}
function compositionEnd() {
  model.composition(false);
  compositionEnded = performance.now();
}
async function removeHistory(word: string) {
  const at = history.value.indexOf(word);
  await model.historyCommand({ kind: 'remove', keyword: word });
  await nextTick();
  if (!errors.history && document.activeElement === document.body) {
    const choices = [
      ...(panel.value?.querySelectorAll<HTMLButtonElement>(
        '.search-tags-section:first-child button[data-search-choice]',
      ) ?? []),
    ].filter((n) => !n.closest('[inert]'));
    (choices[Math.min(at, choices.length - 1)] ?? input.value)?.focus();
  }
}
function key(event: KeyboardEvent) {
  if (event.isComposing || composing.value || event.keyCode === 229) return;
  if (event.key === 'Escape' && open.value) {
    event.preventDefault();
    event.stopPropagation();
    model.close();
    focusInput();
    return;
  }
  if (event.key === 'Enter' && event.target === input.value) {
    event.preventDefault();
    if (!event.repeat) submit();
    return;
  }
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
  const controls = [
    ...(panel.value?.querySelectorAll<HTMLButtonElement>(
      'button[data-search-choice]:not(:disabled)',
    ) ?? []),
  ].filter((n) => {
    const r = n.getBoundingClientRect(),
      clip = n.closest('.search-tags')?.getBoundingClientRect();
    return (
      !n.closest('[inert]') &&
      r.height > 0 &&
      (!clip || (r.top >= clip.top && r.bottom <= clip.bottom))
    );
  });
  if (event.target === input.value) {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      if (!open.value) void show();
      else controls[0]?.focus();
    }
    return;
  }
  const at = controls.indexOf(event.target as HTMLButtonElement);
  if (at < 0) return;
  event.preventDefault();
  let next =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? controls.length - 1
        : at + (event.key === 'ArrowDown' ? 1 : -1);
  if (next < 0) input.value?.focus();
  else {
    next = Math.min(next, controls.length - 1);
    controls[next]?.focus();
    controls[next]?.scrollIntoView({ block: 'nearest' });
  }
}
function outside(event: Event) {
  const target = event.target as Node;
  if (confirm.value || host.value?.contains(target) || panel.value?.contains(target)) return;
  model.close();
}
async function clearHistory() {
  await model.historyCommand({ kind: 'clear' });
  if (!errors.history) confirm.value = false;
}
watch(
  () => [props.account.epoch, props.account.status],
  () => {
    confirm.value = false;
    void model.connect(props.account);
  },
  { immediate: true },
);
watch(
  () => props.pageKey,
  () => {
    model.close();
    if (props.keyword !== undefined) query.value = props.keyword;
  },
);
watch(open, (value) => {
  if (value) place();
});
onMounted(() => {
  document.addEventListener('pointerdown', outside, true);
  document.addEventListener('focusin', outside);
  window.addEventListener('resize', resize);
});
onUnmounted(() => {
  model.dispose();
  cancelAnimationFrame(frame);
  document.removeEventListener('pointerdown', outside, true);
  document.removeEventListener('focusin', outside);
  window.removeEventListener('resize', resize);
});
</script>
<template>
  <div ref="host" class="search-host" @keydown="key">
    <form
      class="searchbox"
      :class="{ 'searchbox-open': open }"
      role="search"
      @submit.prevent="submit()"
    >
      <button
        type="submit"
        class="search-submit"
        aria-label="开始搜索"
        :disabled="account.status !== 'authenticated'"
      >
        <Icon name="search" />
      </button>
      <input
        ref="input"
        :value="query"
        aria-label="搜索音乐"
        role="combobox"
        aria-autocomplete="list"
        aria-haspopup="dialog"
        :aria-expanded="open"
        aria-controls="search-discovery-panel"
        :placeholder="defaultWord?.label || '搜索音乐、寻找好声音'"
        maxlength="256"
        :disabled="account.status !== 'authenticated'"
        @input="model.input(($event.target as HTMLInputElement).value)"
        @focus="onFocus"
        @click="show"
        @compositionstart="model.composition(true)"
        @compositionend="compositionEnd"
      />
      <button v-if="query" type="button" class="icon-button" aria-label="清空搜索" @click="clear">
        <Icon name="close" />
      </button>
    </form>
    <Teleport to="body"
      ><MenuTransition>
        <div
          v-if="open"
          id="search-discovery-panel"
          ref="panel"
          class="search-panel"
          :style="position"
          role="dialog"
          aria-label="音乐搜索"
          @keydown="key"
        >
          <SearchSuggestions
            v-if="searching"
            :query="query.trim()"
            :title="suggestionTitle"
            :recommended="recommended"
            :suggestions="suggestions"
            :pending="!!pending.suggestions"
            :composing="composing"
            :error="errors.suggestions"
            @select="submit"
            @retry="model.load({ kind: 'suggestions', keyword: query.trim() })"
          />
          <template v-else>
            <SearchTags
              title="搜索历史"
              :words="historyWords"
              history
              :busy="pending.history"
              :empty-label="pending.history || errors.history ? '' : '暂无搜索历史'"
              @select="submit"
              @remove="removeHistory"
              @clear="confirm = true"
            />
            <p v-if="pending.history" role="status" class="search-muted">正在读取搜索历史…</p>
            <div v-if="errors.history" class="search-section-error" role="alert">
              <span>{{ errors.history }}</span
              ><button @click="model.historyCommand({ kind: 'read' })">重试搜索历史</button
              ><button @click="confirm = true">清空搜索历史</button>
            </div>
            <SearchTags
              v-if="recommendations.length"
              title="猜你喜欢"
              :words="recommendations"
              @select="submit"
            />
            <p v-if="pending.recommendations" role="status" class="search-muted">正在加载推荐词…</p>
            <div v-if="errors.recommendations" class="search-section-error" role="alert">
              <span>{{ errors.recommendations }}</span
              ><button
                @click="model.load({ kind: 'recommendations', lastQuery: history[0] ?? '' })"
              >
                重试推荐词
              </button>
            </div>
            <p v-if="pending.charts" role="status" class="search-muted">正在加载热搜榜…</p>
            <div v-else-if="errors.charts" class="search-section-error" role="alert">
              <span>{{ errors.charts }}</span
              ><button @click="model.load({ kind: 'charts' })">重试热搜榜</button>
            </div>
            <p v-else-if="!charts.length" class="search-muted">暂无热搜榜</p>
            <SearchCharts
              :charts="charts"
              :pending="pending"
              :errors="errors"
              @select="submit"
              @retry="(id) => model.load({ kind: 'chart', id }, 'chart:' + id)"
            />
          </template>
        </div> </MenuTransition
    ></Teleport>
    <ModalTransition :fallback-focus="() => input ?? null"
      ><SearchHistoryConfirm
        v-if="confirm"
        :busy="!!pending.history"
        :error="errors.history ?? ''"
        @close="confirm = false"
        @confirm="clearHistory"
    /></ModalTransition>
  </div>
</template>
<style src="../../styles/search.css"></style>
