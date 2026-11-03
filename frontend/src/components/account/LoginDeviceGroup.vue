<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue';
import { accountDate, loginDeviceIcon, type LoginDevice } from '../../services/accountDetail';
import { useControlFocus } from '../../composables/useControlFocus';
import Icon from '../Icon.vue';
const props = defineProps<{
  kind: 'signed-in' | 'signed-out';
  title: string;
  emptyLabel: string;
  devices: LoginDevice[];
}>();
const emit = defineEmits<{ detail: [device: LoginDevice, event: MouseEvent] }>();
const heading = ref<HTMLElement>();
const headingId = useId();
const page = ref(0);
const perPage = 6;
const count = computed(() => Math.ceil(props.devices.length / perPage));
const visible = computed(() =>
  props.devices.slice(page.value * perPage, (page.value + 1) * perPage),
);
const focus = useControlFocus(
  () => props.devices,
  () => heading.value,
);
watch(
  () => props.devices,
  () => (page.value = 0),
);
function turn(event: Event, delta: number) {
  void focus.run(event, () => {
    page.value = Math.max(0, Math.min(count.value - 1, page.value + delta));
  });
}
</script>
<template>
  <section class="login-device-group" :data-device-group="kind" :aria-labelledby="headingId">
    <h2 :id="headingId" ref="heading" tabindex="-1">{{ title }} {{ devices.length }}</h2>
    <p v-if="!devices.length" class="login-devices-empty">{{ emptyLabel }}</p>
    <ul v-else class="login-device-grid">
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
              <small v-if="device.current" class="login-device-current">本机</small>
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
    <nav v-if="count > 1" class="login-device-pages" :aria-label="`${title}分页`">
      <button type="button" :disabled="page === 0" @click="turn($event, -1)">上一组</button>
      <span role="status">{{ page + 1 }} / {{ count }}</span>
      <button type="button" :disabled="page + 1 === count" @click="turn($event, 1)">
        查看更多
      </button>
    </nav>
  </section>
</template>
