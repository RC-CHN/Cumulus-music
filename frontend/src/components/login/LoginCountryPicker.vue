<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useId, watch } from 'vue';
import type { LoginCountry } from '../../services/loginPresentation';
import { useVirtualRows } from '../../composables/useVirtualRows';
const props = defineProps<{ countries: LoginCountry[]; alpha: string; disabled: boolean }>();
const emit = defineEmits<{ select: [country: LoginCountry] }>();
const root = ref<HTMLElement>(),
  trigger = ref<HTMLButtonElement>(),
  search = ref<HTMLInputElement>();
const open = ref(false),
  query = ref(''),
  active = ref(0),
  above = ref(false),
  height = ref(315);
const id = useId();
let opening = false;
const selected = computed(() => props.countries.find((row) => row.alpha === props.alpha));
const rows = computed(() => {
  const text = query.value.trim().replace(/^\+/, '').toLowerCase();
  return props.countries.filter((row) =>
    [row.name, row.englishName.toLowerCase(), row.alpha.toLowerCase(), row.calling].some((value) =>
      value.includes(text),
    ),
  );
});
const { container, start, end, onScroll, reset } = useVirtualRows(() => rows.value.length, 36, 3);
const rendered = computed(() => rows.value.slice(start.value, end.value));
function close(focus = false) {
  open.value = false;
  if (focus) trigger.value?.focus({ preventScroll: true });
}
async function ensureActive() {
  await nextTick();
  const list = container.value;
  if (!list) return;
  const top = active.value * 36;
  if (top < list.scrollTop) list.scrollTop = top;
  else if (top + 36 > list.scrollTop + list.clientHeight)
    list.scrollTop = top + 36 - list.clientHeight;
  onScroll();
}
async function show() {
  if (props.disabled) return;
  opening = true;
  const rect = trigger.value!.getBoundingClientRect();
  const dialog = root.value!.closest('dialog')!.getBoundingClientRect();
  const below = Math.min(innerHeight, dialog.bottom) - rect.bottom - 12;
  const before = rect.top - Math.max(0, dialog.top) - 12;
  above.value = below < 200 && before > below;
  height.value = Math.min(315, Math.max(120, above.value ? before : below));
  query.value = '';
  active.value = Math.max(
    0,
    props.countries.findIndex((row) => row.alpha === props.alpha),
  );
  open.value = true;
  await nextTick();
  opening = false;
  if (!open.value || props.disabled || root.value?.closest('[inert]')) return;
  search.value?.focus({ preventScroll: true });
  await ensureActive();
}
function choose(row: LoginCountry) {
  emit('select', row);
  close(true);
}
function key(event: KeyboardEvent) {
  if (event.isComposing || event.keyCode === 229) {
    // The IME owns these keys. Suppress browser form submission or dialog
    // cancellation when WebKit also reports the underlying Enter or Escape.
    if (event.key === 'Enter' || event.key === 'Escape') event.preventDefault();
    return;
  }
  if (event.key === 'Escape') {
    event.preventDefault();
    event.stopPropagation();
    close(true);
  } else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
    event.preventDefault();
    active.value =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? rows.value.length - 1
          : Math.max(
              0,
              Math.min(rows.value.length - 1, active.value + (event.key === 'ArrowDown' ? 1 : -1)),
            );
    void ensureActive();
  } else if (event.key === 'Enter') {
    event.preventDefault();
    const row = rows.value[active.value];
    if (row) choose(row);
  }
}
function outside(event: PointerEvent) {
  if (event.target instanceof Node && !root.value?.contains(event.target)) close();
}
async function blur() {
  await nextTick();
  if (!root.value?.contains(document.activeElement)) close();
}
watch(query, () => {
  if (opening) return;
  active.value = 0;
  reset();
});
watch(
  () => props.disabled,
  (value) => {
    if (value) close();
  },
);
onMounted(() => document.addEventListener('pointerdown', outside, true));
onBeforeUnmount(() => document.removeEventListener('pointerdown', outside, true));
</script>
<template>
  <div ref="root" class="login-country-picker" @focusout="blur">
    <button
      ref="trigger"
      type="button"
      class="login-country-trigger"
      aria-label="国际区号"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-controls="open ? id : undefined"
      :disabled="disabled"
      :title="selected?.name"
      @click="open ? close() : show()"
      @keydown.down.prevent="show"
      @keydown.up.prevent="show"
    >
      +{{ selected?.calling ?? '86' }}<span aria-hidden="true">⌄</span>
    </button>
    <div
      v-if="open"
      class="login-country-menu"
      :class="{ above }"
      :style="{ height: `${height}px` }"
    >
      <input
        ref="search"
        v-model="query"
        type="search"
        aria-label="搜索国家或地区"
        placeholder="搜索地区或区号"
        role="combobox"
        aria-autocomplete="list"
        :aria-expanded="true"
        :aria-controls="id"
        :aria-activedescendant="rows[active] ? `${id}-${rows[active]!.alpha}` : undefined"
        autocomplete="off"
        @keydown="key"
      />
      <div
        :id="id"
        ref="container"
        role="listbox"
        aria-label="国家和地区"
        class="login-country-list"
        @scroll.passive="onScroll"
      >
        <div v-if="start" role="presentation" :style="{ height: `${start * 36}px` }" />
        <div
          v-for="(row, index) in rendered"
          :id="`${id}-${row.alpha}`"
          :key="row.alpha"
          role="option"
          :aria-selected="row.alpha === alpha"
          :data-country="row.alpha"
          class="login-country-option"
          :class="{ active: start + index === active }"
          @pointerdown.prevent
          @click="choose(row)"
        >
          <span>{{ row.name }}</span
          ><span>+{{ row.calling }}</span>
        </div>
        <div
          v-if="end < rows.length"
          role="presentation"
          :style="{ height: `${(rows.length - end) * 36}px` }"
        />
        <p v-if="!rows.length" class="country-empty" role="status">没有匹配的地区</p>
      </div>
    </div>
  </div>
</template>
