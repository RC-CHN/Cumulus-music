<script setup lang="ts">
import { computed, nextTick, ref, useId } from 'vue';
import { accountDate, loginDeviceIcon, type LoginDevice } from '../../services/accountDetail';
import Icon from '../Icon.vue';
const props = defineProps<{
  kind: 'signed-in' | 'signed-out';
  title: string;
  emptyLabel: string;
  devices: LoginDevice[];
  expanded?: boolean;
}>();
const emit = defineEmits<{
  detail: [device: LoginDevice, event: MouseEvent];
  toggle: [expanded: boolean, trigger: HTMLButtonElement];
}>();
const headingId = useId();
const listId = useId();
const list = ref<HTMLUListElement>();
const initialRows = 6;
const expandable = computed(() => props.kind === 'signed-in' && props.devices.length > initialRows);
const visible = computed(() =>
  expandable.value && !props.expanded ? props.devices.slice(0, initialRows) : props.devices,
);
async function toggle(event: MouseEvent) {
  const trigger = event.currentTarget;
  if (!(trigger instanceof HTMLButtonElement) || !expandable.value) return;
  const opening = !props.expanded;
  const keyboard = event.detail === 0 && document.activeElement === trigger;
  emit('toggle', opening, trigger);
  if (!opening || !keyboard) return;
  await nextTick();
  if (props.expanded && document.activeElement === trigger) {
    const firstRevealed =
      list.value?.querySelectorAll<HTMLButtonElement>('.login-device-card')[initialRows];
    firstRevealed?.focus({ preventScroll: true });
    firstRevealed?.scrollIntoView({ block: 'center', inline: 'nearest' });
  }
}
</script>
<template>
  <section class="login-device-group" :data-device-group="kind" :aria-labelledby="headingId">
    <h2 :id="headingId">{{ title }} {{ devices.length }}</h2>
    <p v-if="!devices.length" class="login-devices-empty">{{ emptyLabel }}</p>
    <ul v-else :id="listId" ref="list" class="login-device-grid">
      <li v-for="device in visible" :key="device.id">
        <button
          type="button"
          class="login-device-card"
          :data-device-current="device.current"
          @click="emit('detail', device, $event)"
        >
          <span class="login-device-icon" :title="device.os || undefined"
            ><Icon :name="loginDeviceIcon(device.os)"
          /></span>
          <span class="login-device-card-text">
            <span class="login-device-name">
              <strong>{{ device.name || '未知设备' }}</strong>
              <small v-if="device.current && kind === 'signed-in'" class="login-device-current"
                >本机</small
              >
            </span>
            <span class="login-device-last-active">
              {{ accountDate(device.lastActiveTime) || '活跃时间未提供'
              }}{{ device.location ? ` · ${device.location}` : '' }}
            </span>
          </span>
          <Icon name="forward" class="login-device-forward" />
        </button>
      </li>
    </ul>
    <button
      v-if="expandable"
      type="button"
      class="login-device-expand"
      :aria-expanded="!!expanded"
      :aria-controls="listId"
      @click="toggle"
    >
      {{ expanded ? '收起' : '查看更多' }}<Icon :name="expanded ? 'up' : 'down'" />
    </button>
  </section>
</template>
