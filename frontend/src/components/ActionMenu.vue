<script setup lang="ts">
import MenuTransition from './MenuTransition.vue';
import { nextTick, onUnmounted, ref, useId, watch } from 'vue';
import Icon from './Icon.vue';
import type { MenuItem } from '../services/menu';
const props = withDefaults(
  defineProps<{
    label: string;
    items: MenuItem[];
    icon?: string;
    triggerText?: string;
    disabled?: boolean;
    focusChecked?: boolean;
    contextOnly?: boolean;
    variant?: 'song' | 'speed' | 'quality';
  }>(),
  {
    icon: 'more',
  },
);
const emit = defineEmits<{ select: [key: string]; opened: []; closed: [] }>();
const open = ref(false);
const trigger = ref<HTMLButtonElement>();
const panel = ref<HTMLElement>();
const position = ref({ left: '0px', top: '0px', visibility: 'hidden' as 'hidden' | 'visible' });
const id = useId();
let stop: (() => void) | undefined;
let returnTarget: HTMLElement | undefined;
let displayRevision = 0;
let viewportBounds: { width: number; height: number } | undefined;
let anchorBounds: { left: number; top: number; width: number; height: number } | undefined;
let origin: { x: number; y: number } | undefined;
let resizeObserver: ResizeObserver | undefined;
let followFocus = true;
function rememberAnchor() {
  viewportBounds = { width: innerWidth, height: innerHeight };
  const node = returnTarget ?? trigger.value;
  if (!node?.isConnected) {
    anchorBounds = undefined;
    return;
  }
  const { left, top, width, height } = node.getBoundingClientRect();
  anchorBounds = { left, top, width, height };
}
function anchorUnchanged() {
  const node = returnTarget ?? trigger.value;
  if (!node?.isConnected || !anchorBounds) return false;
  const current = node.getBoundingClientRect();
  return (
    Math.abs(current.left - anchorBounds.left) < 0.5 &&
    Math.abs(current.top - anchorBounds.top) < 0.5 &&
    Math.abs(current.width - anchorBounds.width) < 0.5 &&
    Math.abs(current.height - anchorBounds.height) < 0.5
  );
}
const choices = () => [
  ...(panel.value?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? []),
];
function close(restore = false) {
  displayRevision++;
  anchorBounds = undefined;
  viewportBounds = undefined;
  if (open.value) {
    open.value = false;
    emit('closed');
  }
  resizeObserver?.disconnect();
  resizeObserver = undefined;
  returnTarget?.removeAttribute('aria-controls');
  if (returnTarget?.hasAttribute('aria-haspopup'))
    returnTarget.setAttribute('aria-expanded', 'false');
  if (restore) {
    const target = returnTarget ?? trigger.value;
    if (target && !target.getClientRects().length)
      target.parentElement?.closest<HTMLElement>('[tabindex]')?.focus({ preventScroll: true });
    target?.focus({ preventScroll: true });
  }
}
async function display(last = false, point?: { x: number; y: number }, target?: HTMLElement) {
  if (props.disabled) return;
  close();
  const revision = displayRevision;
  origin = point;
  followFocus = true;
  returnTarget = target ?? trigger.value;
  rememberAnchor();
  if (returnTarget?.hasAttribute('aria-haspopup')) {
    returnTarget.setAttribute('aria-expanded', 'true');
    returnTarget.setAttribute('aria-controls', id);
  }
  open.value = true;
  emit('opened');
  position.value.visibility = 'hidden';
  await nextTick();
  if (revision !== displayRevision || !open.value || (!point && !trigger.value) || !panel.value)
    return;
  place();
  await nextTick();
  if (revision !== displayRevision || !open.value) return;
  rememberAnchor();
  focusChoice(last);
  if (typeof ResizeObserver !== 'undefined') {
    resizeObserver = new ResizeObserver(() => {
      if (!open.value) return;
      place();
      if (followFocus && panel.value?.contains(document.activeElement))
        (document.activeElement as HTMLElement)?.scrollIntoView({ block: 'nearest' });
    });
    observeContent();
  }
}
function observeContent() {
  resizeObserver?.disconnect();
  if (!panel.value) return;
  for (const element of [
    panel.value,
    ...panel.value.children,
    ...panel.value.querySelectorAll('.menu-items button'),
  ])
    resizeObserver?.observe(element);
}
function place() {
  if (!panel.value || !open.value || (!origin && !trigger.value)) return;
  const point = origin;
  const anchor = point
      ? { right: point.x, top: point.y, bottom: point.y }
      : trigger.value!.getBoundingClientRect(),
    box = panel.value.getBoundingClientRect();
  const left = Math.max(
    8,
    Math.min(
      point
        ? point.x
        : props.variant === 'quality'
          ? (anchor.right + trigger.value!.getBoundingClientRect().left - box.width) / 2
          : anchor.right - box.width,
      innerWidth - box.width - 8,
    ),
  );
  const top =
    props.variant !== 'quality' && anchor.bottom + box.height + 8 < innerHeight
      ? anchor.bottom + 6
      : Math.max(8, anchor.top - box.height - 6);
  position.value = { left: `${left}px`, top: `${top}px`, visibility: 'visible' };
}
function focusChoice(last = false) {
  const items = choices();
  const checked = props.focusChecked
    ? items.find((item) => item.getAttribute('aria-checked') === 'true')
    : undefined;
  const focusedItem = (last ? items.at(-1) : (checked ?? items[0])) ?? panel.value;
  focusedItem?.focus({ preventScroll: true });
  if (focusedItem !== panel.value) focusedItem?.scrollIntoView({ block: 'nearest' });
}
watch(
  () =>
    JSON.stringify(
      props.items.map((item) => [item.key, item.checked, item.disabled, item.separator]),
    ),
  async () => {
    if (!open.value) return;
    const revision = displayRevision;
    const wasInside = panel.value?.contains(document.activeElement);
    await nextTick();
    if (revision !== displayRevision || !open.value) return;
    place();
    observeContent();
    if (
      wasInside &&
      (!panel.value?.contains(document.activeElement) ||
        (document.activeElement as HTMLButtonElement)?.disabled)
    )
      focusChoice();
    else if (wasInside && followFocus)
      (document.activeElement as HTMLElement)?.scrollIntoView({ block: 'nearest' });
  },
);
const show = (last = false) => display(last);
const showAt = (x: number, y: number, target?: HTMLElement) => display(false, { x, y }, target);
function select(key: string) {
  close(true);
  emit('select', key);
}
function keydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault();
    event.stopPropagation();
    close(true);
    return;
  }
  if (event.key === 'Tab') {
    close(true);
    return;
  }
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
  followFocus = true;
  event.preventDefault();
  const items = choices();
  const current = items.indexOf(document.activeElement as HTMLButtonElement);
  const target =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? items.length - 1
        : (current + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
  items[target]?.focus({ preventScroll: true });
  items[target]?.scrollIntoView({ block: 'nearest' });
}
watch(
  () => props.disabled,
  (disabled) => {
    if (disabled) close();
  },
);
watch(open, (active) => {
  stop?.();
  stop = undefined;
  if (!active) return;
  const outside = (event: PointerEvent) => {
    if (
      !panel.value?.contains(event.target as Node) &&
      !trigger.value?.contains(event.target as Node)
    )
      close();
  };
  const scroll = (event: Event) => {
    if (panel.value?.contains(event.target as Node)) return;
    if (anchorUnchanged()) return;
    close();
  };
  const resize = () => {
    // Some native hosts report a resize even when CSS dimensions did not
    // change. Keep a correctly anchored menu open for that notification.
    if (
      viewportBounds?.width === innerWidth &&
      viewportBounds.height === innerHeight &&
      anchorUnchanged()
    )
      return;
    close();
  };
  document.addEventListener('pointerdown', outside, true);
  document.addEventListener('scroll', scroll, true);
  window.addEventListener('resize', resize);
  stop = () => {
    document.removeEventListener('pointerdown', outside, true);
    document.removeEventListener('scroll', scroll, true);
    window.removeEventListener('resize', resize);
  };
});
onUnmounted(() => {
  close();
  stop?.();
});
defineExpose({ show, showAt, close });
</script>
<template>
  <span class="action-menu-host">
    <button
      v-if="!contextOnly"
      type="button"
      ref="trigger"
      class="icon-button menu-trigger"
      :aria-label="label"
      :data-tooltip="triggerText ? `${label}：${triggerText}` : label"
      :aria-describedby="triggerText ? `${id}-value` : undefined"
      aria-haspopup="menu"
      :aria-expanded="open"
      :aria-controls="open ? id : undefined"
      :disabled="disabled"
      @click="open ? close() : show()"
      @keydown.down.prevent="show()"
      @keydown.up.prevent="show(true)"
    >
      <span v-if="triggerText">{{ triggerText }}</span>
      <Icon v-else :name="icon" />
    </button>
    <span v-if="triggerText" :id="`${id}-value`" class="sr-only">当前{{ triggerText }}</span>
    <Teleport to="body">
      <MenuTransition>
        <div
          v-if="open"
          :id="id"
          ref="panel"
          class="action-menu"
          :class="{
            'song-action-menu': variant === 'song',
            'speed-action-menu': variant === 'speed',
            'quality-action-menu': variant === 'quality',
          }"
          role="menu"
          tabindex="-1"
          :aria-label="label"
          :style="position"
          @keydown="keydown"
          @wheel.passive="followFocus = false"
          @touchmove.passive="followFocus = false"
          @pointerdown="
            ($event.target as HTMLElement)?.classList.contains('menu-items') &&
            (followFocus = false)
          "
        >
          <slot name="header" />
          <div class="menu-items">
            <template v-for="item in items" :key="item.key">
              <div v-if="item.separator" role="separator" class="menu-separator" />
              <button
                v-else
                type="button"
                :role="item.checked === undefined ? 'menuitem' : 'menuitemradio'"
                :aria-checked="item.checked"
                :disabled="item.disabled"
                :title="item.hint"
                :class="{ danger: item.danger, checked: item.checked }"
                @click="select(item.key)"
              >
                <Icon v-if="item.icon" :name="item.icon" /><slot name="item" :item="item"
                  ><span>{{ item.label }}</span></slot
                ><Icon v-if="item.checked" class="menu-check" name="check" />
              </button>
            </template>
          </div>
          <slot name="footer" />
        </div>
      </MenuTransition>
    </Teleport>
  </span>
</template>
