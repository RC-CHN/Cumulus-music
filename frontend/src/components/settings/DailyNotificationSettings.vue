<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue';
import { failure } from '../../services/contracts';
import { useCore } from '../../services/core';

const props = defineProps<{ epoch: string }>();
const client = useCore();
const enabled = ref<boolean>();
const busy = ref(false);
const issue = ref('');
let generation = 0;
let alive = true;
let stop: (() => void) | undefined;

async function read() {
  const owner = props.epoch;
  const ticket = ++generation;
  enabled.value = undefined;
  issue.value = '';
  busy.value = true;
  try {
    const value = await client.dailyNotificationEnabled(owner);
    if (alive && generation === ticket && props.epoch === owner) enabled.value = value;
  } catch (error) {
    if (alive && generation === ticket) issue.value = failure(error).message;
  } finally {
    if (alive && generation === ticket) busy.value = false;
  }
}

async function save(event: Event) {
  const box = event.target as HTMLInputElement;
  const desired = box.checked;
  box.checked = enabled.value === true;
  if (busy.value || enabled.value === undefined) return;
  const owner = props.epoch;
  const ticket = ++generation;
  issue.value = '';
  busy.value = true;
  try {
    const value = await client.setDailyNotificationEnabled(owner, desired);
    if (alive && generation === ticket && props.epoch === owner) enabled.value = value;
  } catch (error) {
    if (alive && generation === ticket) issue.value = failure(error).message;
  } finally {
    if (alive && generation === ticket) busy.value = false;
  }
}

watch(
  () => props.epoch,
  () => void read(),
);
onMounted(async () => {
  try {
    const unlisten = await client.onDailyNotificationPreference(({ epoch, value }) => {
      if (!alive || epoch !== props.epoch) return;
      generation++;
      enabled.value = value;
      busy.value = false;
    });
    if (!alive) {
      unlisten();
      return;
    }
    stop = unlisten;
  } catch {
    // The local preference is still usable without a live observer.
  }
  if (alive) await read();
});
onUnmounted(() => {
  alive = false;
  stop?.();
});
</script>

<template>
  <section class="settings-section" aria-labelledby="settings-desktop-notifications">
    <h2 id="settings-desktop-notifications" tabindex="-1">桌面通知</h2>
    <div class="settings-section-content">
      <label class="check-setting">
        <input
          type="checkbox"
          aria-label="每日推荐"
          data-daily-notification-preference
          :checked="enabled"
          :disabled="enabled === undefined || busy"
          @change="save"
        />
        每日推荐
      </label>
      <p class="settings-note">仅在 Cumulus 主窗口中显示。</p>
      <p v-if="issue" class="catalog-error" role="status">{{ issue }}</p>
    </div>
  </section>
</template>
