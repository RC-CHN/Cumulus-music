<script setup lang="ts">
import { onMounted, onUnmounted, reactive, watch } from 'vue';
import { failure } from '../../services/contracts';
import { useCore } from '../../services/core';
import type { DailyNotificationKind } from '../../services/dailyNotification';

const props = defineProps<{ epoch: string }>();
const client = useCore();
const kinds = ['daily', 'copyright'] as const;
const names = { daily: '每日推荐', copyright: '版权回归' };
const row = () => ({ enabled: null as boolean | null, busy: false, issue: '', generation: 0 });
const state = reactive({ daily: row(), copyright: row() });
const stops: (() => void)[] = [];
let alive = true;

function current(owner: string, kind: DailyNotificationKind, ticket: number) {
  return alive && props.epoch === owner && state[kind].generation === ticket;
}

async function read(kind: DailyNotificationKind, reset: boolean) {
  const entry = state[kind];
  if (!reset && entry.busy) return;
  const owner = props.epoch;
  const ticket = ++entry.generation;
  if (reset) entry.enabled = null;
  entry.issue = '';
  entry.busy = true;
  try {
    const value = await client.dailyNotificationEnabled(owner, kind);
    if (current(owner, kind, ticket)) entry.enabled = value;
  } catch (error) {
    if (current(owner, kind, ticket)) entry.issue = failure(error).message;
  } finally {
    if (current(owner, kind, ticket)) entry.busy = false;
  }
}

async function save(event: Event, kind: DailyNotificationKind) {
  const entry = state[kind];
  const box = event.target as HTMLInputElement;
  const desired = box.checked;
  box.checked = entry.enabled === true;
  if (entry.busy || entry.enabled === null) return;
  const owner = props.epoch;
  const ticket = ++entry.generation;
  entry.issue = '';
  entry.busy = true;
  try {
    const value = await client.setDailyNotificationEnabled(owner, kind, desired);
    if (current(owner, kind, ticket)) entry.enabled = value;
  } catch (error) {
    if (current(owner, kind, ticket)) entry.issue = failure(error).message;
  } finally {
    if (current(owner, kind, ticket)) entry.busy = false;
  }
}

const reload = () => Promise.all(kinds.map((kind) => read(kind, true)));
watch(
  () => props.epoch,
  () => void reload(),
);
onMounted(async () => {
  const listen = [
    client.onDailyNotificationPreference(({ epoch, kind, value }) => {
      if (!alive || epoch !== props.epoch) return;
      const entry = state[kind];
      entry.generation++;
      entry.enabled = value;
      entry.busy = false;
    }),
    client.onPcPresentationChanged((view) => {
      if (alive && view.epoch === props.epoch) void read('copyright', false);
    }),
  ];
  for (const result of await Promise.allSettled(listen)) {
    if (result.status !== 'fulfilled') continue;
    if (alive) stops.push(result.value);
    else result.value();
  }
  if (alive) await reload();
});
onUnmounted(() => {
  alive = false;
  for (const stop of stops) stop();
});
</script>

<template>
  <section class="settings-section" aria-labelledby="settings-desktop-notifications">
    <h2 id="settings-desktop-notifications" tabindex="-1">桌面通知</h2>
    <div class="settings-section-content">
      <template v-for="kind in kinds" :key="kind">
        <label v-if="kind === 'daily' || state[kind].enabled !== null" class="settings-checkbox">
          <input
            type="checkbox"
            :aria-label="names[kind]"
            :data-daily-notification-preference="kind"
            :checked="state[kind].enabled === true"
            :disabled="state[kind].enabled === null || state[kind].busy"
            @change="save($event, kind)"
          />
          <span>{{ names[kind] }}</span>
        </label>
        <p v-if="state[kind].issue" class="catalog-error" role="status">{{ state[kind].issue }}</p>
      </template>
      <p class="settings-note">仅在 Cumulus 主窗口中显示。</p>
    </div>
  </section>
</template>
