<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useCore } from '../services/core';
import {
  commentReportChoices,
  resourceKey,
  type CommentItem,
  type CommentResource,
} from '../services/comments';
import type { ReportChoice, ReportCommand } from '../services/profileReport';
import ModerationReportDialog from './ModerationReportDialog.vue';

const props = defineProps<{
  epoch: string;
  resource: CommentResource;
  item: CommentItem;
  canDelete: boolean;
}>();
const emit = defineEmits<{ expired: []; remove: [] }>();
const core = useCore();
const holder = ref<HTMLElement>();
const trigger = ref<HTMLButtonElement>();
const popup = ref<HTMLElement>();
const menu = ref<'context' | 'reasons' | 'own'>();
const choice = ref<ReportChoice | null>(null);
const visited = ref(false);
const position = ref({ left: '0px', top: '0px' });
const leftFlyout = ref(false);
const scope = computed(() => resourceKey(props.resource));
const dispatch = computed(() => {
  const resource = props.resource;
  return (epoch: string, uid: string, command: ReportCommand) =>
    core.commentReport(epoch, resource, uid, command);
});
const reportable = computed(() => !!props.item.reportable && !props.item.owned && !!props.epoch);
let context = false;
let anchor: HTMLElement | null = null;
function returnFocus() {
  return (anchor?.isConnected ? anchor : trigger.value) ?? null;
}
function close(restore = false) {
  menu.value = undefined;
  if (restore) returnFocus()?.focus({ preventScroll: true });
}
function reposition(x: number, y: number) {
  const left = Math.max(8, Math.min(x, innerWidth - 248));
  leftFlyout.value = left + 440 > innerWidth;
  position.value = {
    left: `${left}px`,
    top: `${Math.max(8, Math.min(y, innerHeight - (menu.value === 'reasons' ? 232 : 60)))}px`,
  };
}
const activeMenu = () => popup.value?.querySelector<HTMLElement>('[data-comment-report-menu]');
async function show(kind: NonNullable<typeof menu.value>, x: number, y: number) {
  if (kind !== 'own') visited.value = true;
  menu.value = kind;
  reposition(x, y);
  await nextTick();
  activeMenu()?.querySelector<HTMLButtonElement>('button')?.focus({ preventScroll: true });
}
function openFromButton() {
  if (!reportable.value) return;
  if (menu.value) return close(true);
  if (!trigger.value) return;
  anchor = trigger.value;
  context = false;
  const rect = anchor.getBoundingClientRect();
  void show('reasons', rect.left, rect.bottom + 4);
}
function contextMenu(event: MouseEvent | KeyboardEvent) {
  if (!reportable.value && !props.canDelete) return;
  event.preventDefault();
  event.stopPropagation();
  anchor =
    trigger.value ??
    holder.value
      ?.closest('.comment-card')
      ?.querySelector<HTMLButtonElement>('[data-own-comment-delete]') ??
    null;
  context = true;
  const rect = anchor?.getBoundingClientRect();
  const x = event instanceof MouseEvent && event.clientX ? event.clientX : (rect?.left ?? 0);
  const y = event instanceof MouseEvent && event.clientY ? event.clientY : (rect?.bottom ?? 0);
  void show(reportable.value ? 'context' : 'own', x, y);
}
function reasonMenu() {
  const rect = popup.value?.getBoundingClientRect();
  void show('reasons', rect?.left ?? 0, rect?.top ?? 0);
}
function keydown(event: KeyboardEvent) {
  if (
    event.key === 'Escape' ||
    (event.key === 'ArrowLeft' && menu.value === 'reasons' && context)
  ) {
    event.preventDefault();
    event.stopPropagation();
    if (menu.value === 'reasons' && context) {
      const rect = popup.value?.getBoundingClientRect();
      void show('context', rect?.left ?? 0, rect?.top ?? 0);
    } else close(true);
  } else if (event.key === 'ArrowRight' && menu.value === 'context') {
    event.preventDefault();
    reasonMenu();
  } else if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
    event.preventDefault();
    const buttons = Array.from(activeMenu()?.querySelectorAll('button') ?? []);
    if (!buttons.length) return;
    const current = buttons.findIndex((node) => node === document.activeElement);
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? buttons.length - 1
          : (current + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length;
    buttons[next]?.focus({ preventScroll: true });
  } else if (event.key === 'Tab') close(true);
}
function pick(value: ReportChoice) {
  if (!reportable.value) return;
  close(true);
  choice.value = value;
}
function outside(event: Event) {
  if (
    event.target instanceof Node &&
    !popup.value?.contains(event.target) &&
    !holder.value?.contains(event.target)
  )
    close();
}
function viewportChanged() {
  close();
}
function remove() {
  close(true);
  emit('remove');
}
watch(menu, (value, previous, cleanup) => {
  if (!value) return;
  window.addEventListener('pointerdown', outside, true);
  window.addEventListener('resize', viewportChanged);
  window.addEventListener('scroll', viewportChanged, true);
  cleanup(() => {
    window.removeEventListener('pointerdown', outside, true);
    window.removeEventListener('resize', viewportChanged);
    window.removeEventListener('scroll', viewportChanged, true);
  });
});
watch(
  () => [props.epoch, scope.value, props.item.id, reportable.value],
  () => {
    close();
    choice.value = null;
  },
  { flush: 'sync' },
);
onBeforeUnmount(() => close());
defineExpose({ contextMenu });
</script>

<template>
  <span ref="holder" class="comment-report-menu-anchor">
    <button
      v-if="reportable"
      ref="trigger"
      class="text-button"
      type="button"
      data-comment-report
      aria-haspopup="menu"
      :aria-expanded="!!menu"
      @click="openFromButton"
      @keydown.down.prevent="openFromButton"
      @keydown.shift.f10.prevent="contextMenu"
    >
      举报
    </button>
  </span>
  <Teleport to="body">
    <div
      v-if="menu"
      ref="popup"
      class="comment-report-popover"
      :style="position"
      @keydown="keydown"
      @contextmenu.prevent
    >
      <div
        v-if="menu === 'context' || (menu === 'reasons' && context)"
        role="menu"
        class="comment-report-menu"
        aria-label="评论操作"
        :data-comment-report-menu="menu === 'context' ? '' : undefined"
      >
        <button
          type="button"
          role="menuitem"
          aria-haspopup="menu"
          :aria-expanded="menu === 'reasons'"
          @click="reasonMenu"
          @pointerenter="menu === 'context' && reasonMenu()"
        >
          举报 <span aria-hidden="true">›</span>
        </button>
      </div>
      <div
        v-else-if="menu === 'own'"
        role="menu"
        class="comment-report-menu"
        aria-label="评论操作"
        data-comment-report-menu
      >
        <button type="button" role="menuitem" @click="remove">删除</button>
      </div>
      <div
        v-if="menu === 'reasons'"
        role="menu"
        class="comment-report-menu"
        aria-label="举报原因"
        data-comment-report-menu
        :class="{
          'comment-report-flyout': context,
          'comment-report-flyout-left': context && leftFlyout,
        }"
      >
        <button
          v-for="[value, label] in commentReportChoices"
          :key="value"
          type="button"
          role="menuitem"
          @click="pick(value)"
        >
          {{ label }} <span v-if="value === 'other'" aria-hidden="true">›</span>
        </button>
      </div>
    </div>
  </Teleport>
  <ModerationReportDialog
    v-if="visited"
    :epoch="epoch"
    :uid="item.id"
    :scope="scope"
    :choice="choice"
    :choices="commentReportChoices"
    :target-label="`举报 ${item.author?.name || '云音乐用户'} 的评论`"
    duplicate-message="重复举报"
    :return-focus="returnFocus"
    :dispatch="dispatch"
    @close="choice = null"
    @expired="emit('expired')"
  />
</template>

<style scoped>
.comment-report-menu-anchor:empty {
  display: none;
}
.comment-report-popover {
  position: fixed;
  z-index: 60;
}
.comment-report-flyout {
  position: absolute;
  inset: 0 auto auto 100%;
}
.comment-report-flyout-left {
  inset: 0 100% auto auto;
}
.comment-report-menu {
  min-width: 214px;
  max-width: calc(100vw - 16px);
  padding: 4px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface);
  box-shadow: var(--shadow);
}
.comment-report-menu button {
  display: flex;
  width: 100%;
  align-items: center;
  justify-content: space-between;
  min-height: 32px;
  padding: 4px 10px;
  border: 0;
  border-radius: 6px;
  color: var(--text);
  background: transparent;
  text-align: start;
}
.comment-report-menu button:hover,
.comment-report-menu button:focus-visible {
  background: var(--surface-alt);
}
</style>
