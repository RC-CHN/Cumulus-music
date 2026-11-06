<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useId, watch } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { failure } from '../services/contracts';
import type { SongSpeedSelection, SongSpeedView } from '../services/player';

const props = defineProps<{
  player: PlayerModel;
  observation: SongSpeedView;
  entryId: string;
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
const open = ref(false);
const pending = ref(false);
const issue = ref('');
const draft = ref<number>();
let generation = 0;
let queued: SongSpeedSelection | undefined;
const custom = computed(() => draft.value ?? props.observation.customTenths);
const rate = computed(() => props.player.view.value?.speed ?? 1);

function close(restore = false) {
  open.value = false;
  if (restore) trigger.value?.focus({ preventScroll: true });
}

function key(event: KeyboardEvent) {
  if (event.key !== 'Escape') return;
  if (!open.value) return;
  event.preventDefault();
  event.stopPropagation();
  close(true);
}

async function toggle() {
  if (open.value) {
    close(true);
    return;
  }
  open.value = true;
  await nextTick();
  if (open.value)
    (
      panel.value?.querySelector<HTMLElement>('[aria-pressed="true"]') ??
      panel.value?.querySelector<HTMLElement>('button')
    )?.focus({ preventScroll: true });
}

watch(open, (active, _, cleanup) => {
  if (!active) return;
  function outside(event: PointerEvent) {
    if (!root.value?.contains(event.target as Node)) close();
  }
  document.addEventListener('pointerdown', outside, true);
  cleanup(() => document.removeEventListener('pointerdown', outside, true));
});

function invalidate() {
  generation++;
  queued = undefined;
  pending.value = false;
  draft.value = undefined;
  issue.value = '';
  close();
}

watch(
  [
    () => props.player.view.value?.epoch,
    () => props.observation.revision,
    () => props.entryId,
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
    @keydown="key"
    :data-song-speed-save-state="pending ? 'pending' : issue ? 'failed' : 'idle'"
  >
    <button
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
    <div
      v-if="open"
      :id="id"
      ref="panel"
      class="song-speed-panel"
      role="dialog"
      aria-label="歌曲播放倍速"
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
    </div>
    <span v-if="issue || observation.syncFailed" class="song-speed-notice" role="status">
      {{ issue ? `未能更改播放速度：${issue}` : '未能同步播放速度' }}
    </span>
  </span>
</template>

<style scoped>
.song-speed-control {
  position: relative;
  display: inline-flex;
  flex-shrink: 0;
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
</style>
