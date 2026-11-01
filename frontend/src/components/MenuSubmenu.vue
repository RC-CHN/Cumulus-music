<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue';
import Icon from './Icon.vue';
import type { MenuLeaf } from '../services/menu';
const props = defineProps<{
  id: string;
  label: string;
  items: MenuLeaf[];
  anchor: HTMLElement;
  compact: boolean;
  takeFocus: boolean;
}>();
const emit = defineEmits<{
  select: [key: string, target: HTMLElement];
  close: [restore: boolean];
  dismiss: [];
}>();
const panel = ref<HTMLElement>();
const position = ref({ left: '0px', top: '0px', visibility: 'hidden' as 'hidden' | 'visible' });
const choices = () => [
  ...(panel.value?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? []),
];
function place() {
  if (!panel.value || !props.anchor.isConnected) return;
  const a = props.anchor.getBoundingClientRect(),
    b = panel.value.getBoundingClientRect();
  const root = props.anchor.closest('.action-menu')?.getBoundingClientRect() ?? a;
  const left =
    root.right + b.width + 4 <= innerWidth - 8 ? root.right + 4 : root.left - b.width - 4;
  position.value = {
    left: `${Math.max(8, Math.min(left, innerWidth - b.width - 8))}px`,
    top: `${Math.max(8, Math.min(a.top, innerHeight - b.height - 8))}px`,
    visibility: 'visible',
  };
}
function focus() {
  const nodes = choices();
  const target =
    nodes.find((n) => n.getAttribute('aria-checked') === 'true') ?? nodes[0] ?? panel.value;
  target?.focus({ preventScroll: true });
  target?.scrollIntoView({ block: 'nearest' });
}
onMounted(async () => {
  await nextTick();
  place();
  await nextTick();
  if (props.takeFocus) focus();
});
watch(
  () => props.items,
  async () => {
    const active = panel.value?.contains(document.activeElement);
    await nextTick();
    place();
    if (
      active &&
      (!panel.value?.contains(document.activeElement) ||
        (document.activeElement as HTMLButtonElement)?.disabled)
    )
      focus();
  },
);
function key(event: KeyboardEvent) {
  if (['Escape', 'ArrowLeft'].includes(event.key)) {
    event.preventDefault();
    event.stopPropagation();
    emit('close', true);
    return;
  }
  if (event.key === 'Tab') {
    emit('dismiss');
    return;
  }
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  event.stopPropagation();
  const nodes = choices(),
    index = nodes.indexOf(document.activeElement as HTMLButtonElement);
  const next =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? nodes.length - 1
        : (index + (event.key === 'ArrowDown' ? 1 : -1) + nodes.length) % nodes.length;
  nodes[next]?.focus({ preventScroll: true });
  nodes[next]?.scrollIntoView({ block: 'nearest' });
}
defineExpose({ contains: (target: Node) => !!panel.value?.contains(target), focus, place });
</script>
<template>
  <div
    ref="panel"
    :id="id"
    class="action-menu action-submenu"
    :class="{ 'mini-action-menu': compact }"
    :aria-label="label"
    role="menu"
    tabindex="-1"
    :style="position"
    @keydown="key"
  >
    <template v-for="item in items" :key="item.key">
      <div v-if="item.separator" role="separator" class="menu-separator" />
      <button
        v-else
        type="button"
        :data-menu-key="item.key"
        :role="
          item.checked === undefined
            ? 'menuitem'
            : item.checkKind === 'checkbox'
              ? 'menuitemcheckbox'
              : 'menuitemradio'
        "
        :aria-checked="item.checked"
        :disabled="item.disabled"
        :title="item.hint"
        :class="{ danger: item.danger, checked: item.checked }"
        @click="emit('select', item.key, $event.currentTarget as HTMLElement)"
      >
        <Icon v-if="item.icon" :name="item.icon" /><span>{{ item.label }}</span
        ><Icon v-if="item.checked" class="menu-check" name="check" />
      </button>
    </template>
  </div>
</template>
