<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useId, watch } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { failure } from '../services/contracts';
import { songSpeedMoreBounds } from '../services/songSpeedMore';
import MenuTransition from './MenuTransition.vue';
import type { SongSpeedSelection, SongSpeedView } from '../services/player';

const props = defineProps<{
  player: PlayerModel;
  observation: SongSpeedView;
  entryId: string;
  menuOnly?: boolean;
}>();
const presets = [
  { tenths: 20, label: '2.0' },
  { tenths: 15, label: '1.5' },
  { tenths: 10, label: '1.0（正常）' },
  { tenths: 7, label: '0.7' },
  { tenths: 5, label: '0.5' },
];
const id = useId();
const root = ref<HTMLElement>();
const trigger = ref<HTMLButtonElement>();
const panel = ref<HTMLElement>();
const menuAnchor = ref<HTMLElement>();
const menuBounds = ref<ReturnType<typeof songSpeedMoreBounds>>();
const open = ref(false);
const pending = ref(false);
const issue = ref('');
const draft = ref<number>();
let generation = 0;
let queued: SongSpeedSelection | undefined;
const custom = computed(() => draft.value ?? props.observation.customTenths);
const rate = computed(() => props.player.view.value?.speed ?? 1);
const panelStyle = computed(() => {
  const bounds = menuBounds.value;
  if (!props.menuOnly || !bounds) return;
  return {
    left: `${bounds.left}px`,
    top: `${bounds.top}px`,
    width: `${bounds.width}px`,
    height: `${bounds.height}px`,
  };
});
const noticeStyle = computed(() => {
  const bounds = menuBounds.value;
  if (!props.menuOnly || !bounds) return;
  return { left: `${bounds.left}px`, width: `${bounds.width}px`, top: `${bounds.top}px` };
});

function close(restore = false) {
  open.value = false;
  const target = props.menuOnly ? menuAnchor.value : trigger.value;
  if (restore && target?.isConnected && !target.closest('[inert]'))
    target.focus({ preventScroll: true });
}

function key(event: KeyboardEvent) {
  if (event.key !== 'Escape') return;
  if (!open.value) return;
  event.preventDefault();
  event.stopPropagation();
  close(true);
}

function place() {
  const anchor = menuAnchor.value;
  if (!anchor?.isConnected || !anchor.getClientRects().length || anchor.closest('[inert]')) {
    close();
    return false;
  }
  menuBounds.value = songSpeedMoreBounds(anchor.getBoundingClientRect(), innerWidth, innerHeight);
  return true;
}

async function show() {
  open.value = true;
  await nextTick();
  if (open.value)
    (
      panel.value?.querySelector<HTMLElement>('[aria-pressed="true"]') ??
      panel.value?.querySelector<HTMLElement>('button')
    )?.focus({ preventScroll: true });
}

function toggle() {
  if (open.value) close(true);
  else void show();
}

function showAt(anchor: HTMLElement) {
  if (!props.menuOnly || !props.player.ready.value) return;
  menuAnchor.value = anchor;
  if (place()) void show();
}
defineExpose({ showAt });

function leaveFocus(event: FocusEvent) {
  if (
    props.menuOnly &&
    event.relatedTarget instanceof Node &&
    !panel.value?.contains(event.relatedTarget)
  )
    close();
}

watch(open, (active, _, cleanup) => {
  if (!active) return;
  function outside(event: PointerEvent) {
    if (
      !panel.value?.contains(event.target as Node) &&
      (props.menuOnly || !root.value?.contains(event.target as Node))
    )
      close();
  }
  document.addEventListener('pointerdown', outside, true);
  if (props.menuOnly) {
    window.addEventListener('resize', place);
    document.addEventListener('scroll', place, true);
  }
  cleanup(() => {
    document.removeEventListener('pointerdown', outside, true);
    window.removeEventListener('resize', place);
    document.removeEventListener('scroll', place, true);
  });
});

function invalidate() {
  generation++;
  queued = undefined;
  pending.value = false;
  draft.value = undefined;
  issue.value = '';
  close();
  menuAnchor.value = undefined;
  menuBounds.value = undefined;
}

watch(
  [
    () => props.player.view.value?.epoch,
    () => props.observation.revision,
    () => props.entryId,
    () => props.player.view.value?.playId,
    () => props.menuOnly,
    props.player.ready,
  ],
  invalidate,
  { flush: 'sync' },
);
onBeforeUnmount(invalidate);

async function choose(selection: SongSpeedSelection) {
  if (!props.player.ready.value) return;
  if (selection.kind === 'preset') close(true);
  else draft.value = selection.tenths;
  queued = selection;
  if (pending.value) return;
  const owner = generation;
  pending.value = true;
  issue.value = '';
  const expectedEntry = props.entryId;
  const expectedRevision = props.observation.revision;
  try {
    while (owner === generation && queued) {
      const selected = queued;
      queued = undefined;
      await props.player.configureSongSpeed({
        kind: 'song_speed',
        expected_entry: expectedEntry,
        expected_revision: expectedRevision,
        selection: selected,
      });
    }
  } catch (error) {
    if (owner !== generation) return;
    queued = undefined;
    const result = failure(error);
    if (result.code !== 'stale') issue.value = result.message;
  } finally {
    if (owner === generation) {
      pending.value = false;
      draft.value = undefined;
    }
  }
}

function slide(event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  void choose({ kind: 'custom', tenths: Math.round(input.valueAsNumber * 10) });
}
</script>

<template>
  <span
    ref="root"
    class="song-speed-control"
    :class="{ 'is-menu': menuOnly }"
    @keydown="key"
    :data-song-speed-save-state="pending ? 'pending' : issue ? 'failed' : 'idle'"
  >
    <button
      v-if="!menuOnly"
      ref="trigger"
      class="song-speed-trigger"
      type="button"
      aria-label="播放速度"
      aria-haspopup="dialog"
      :aria-expanded="open"
      :aria-controls="open ? id : undefined"
      :disabled="!player.ready.value"
      @click="toggle"
    >
      {{ rate === 1 ? '播放倍速' : `播放倍速：${rate}X` }}
    </button>
    <Teleport to="body" :disabled="!menuOnly">
      <MenuTransition>
        <div
          v-if="open"
          :id="id"
          ref="panel"
          class="song-speed-panel"
          :class="{ 'is-menu': menuOnly }"
          :style="panelStyle"
          role="dialog"
          aria-label="歌曲播放倍速"
          @keydown="key"
          @focusout="leaveFocus"
        >
          <button
            type="button"
            class="song-speed-row"
            :aria-pressed="observation.customSelected"
            @click="choose({ kind: 'custom', tenths: custom })"
          >
            自定义 ({{ (custom / 10).toFixed(1) }})
          </button>
          <input
            type="range"
            aria-label="自定义播放倍速"
            min="0.5"
            max="2"
            step="0.1"
            :value="custom / 10"
            :aria-valuetext="`${(custom / 10).toFixed(1)} 倍`"
            @input="slide"
          />
          <div role="group" aria-label="预设倍速">
            <button
              v-for="{ tenths, label } in presets"
              :key="tenths"
              class="song-speed-row"
              type="button"
              :aria-pressed="!observation.customSelected && rate === tenths / 10"
              @click="choose({ kind: 'preset', tenths })"
            >
              {{ label }}
            </button>
          </div>
          <p
            v-if="menuOnly && (issue || observation.syncFailed)"
            class="song-speed-menu-issue"
            role="status"
          >
            {{ issue ? `未能更改播放速度：${issue}` : '未能同步播放速度' }}
          </p>
        </div>
      </MenuTransition>
      <span
        v-if="(issue || observation.syncFailed) && (!menuOnly || (!open && menuBounds))"
        class="song-speed-notice"
        :class="{ 'is-menu': menuOnly }"
        :style="noticeStyle"
        role="status"
      >
        {{ issue ? `未能更改播放速度：${issue}` : '未能同步播放速度' }}
      </span>
    </Teleport>
  </span>
</template>

<style scoped>
.song-speed-control {
  position: relative;
  display: inline-flex;
  flex-shrink: 0;
}
.song-speed-control.is-menu {
  display: contents;
}
.song-speed-trigger {
  white-space: nowrap;
}
.song-speed-panel {
  position: absolute;
  z-index: 1;
  inset-block-end: calc(100% + 8px);
  inset-inline-end: 0;
  box-sizing: border-box;
  width: 218px;
  max-width: calc(100vw - 16px);
  max-height: min(255px, calc(100vh - 92px));
  overflow: auto;
  padding: 6px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--surface);
  color: var(--text);
  box-shadow: 0 6px 24px #0004;
}
.song-speed-panel.is-menu {
  position: fixed;
  inset: auto;
  z-index: 71;
  max-height: none;
  padding: 8px 0;
}
.song-speed-panel.is-menu .song-speed-row {
  min-height: 34px;
  padding-inline: 40px 16px;
  text-align: start;
  font-size: 13px;
}
.song-speed-panel.is-menu input {
  width: calc(100% - 80px);
  margin: 0 40px 5px;
}
.song-speed-menu-issue {
  padding-inline: 10px;
  color: var(--accent);
  overflow-wrap: anywhere;
}
.song-speed-row {
  position: relative;
  display: block;
  width: 100%;
  min-height: 31px;
  border: 0;
  background: transparent;
  color: inherit;
  text-align: center;
}
.song-speed-row[aria-pressed='true'] {
  color: var(--accent);
}
.song-speed-row[aria-pressed='true']::before {
  content: '✓';
  position: absolute;
  inset-inline-start: 13px;
}
.song-speed-row:hover {
  background: var(--accent-soft);
}
.song-speed-panel input {
  display: block;
  width: calc(100% - 24px);
  margin: 0 12px 5px;
  accent-color: var(--accent);
}
.song-speed-notice {
  position: absolute;
  inset-block-end: 100%;
  inset-inline-end: 0;
  z-index: 2;
  width: 218px;
  padding: 6px;
  border-radius: 6px;
  color: var(--accent);
  background: var(--surface);
}
.song-speed-notice.is-menu {
  position: fixed;
  inset: auto;
  z-index: 71;
  box-sizing: border-box;
}
</style>
