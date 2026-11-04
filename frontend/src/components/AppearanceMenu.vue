<script setup lang="ts">
import { computed } from 'vue';
import ActionMenu from './ActionMenu.vue';
import { useAppearance } from '../composables/useAppearance';
import Icon from './Icon.vue';
const props = defineProps<{ centerOpen: boolean; disabled: boolean }>();
const emit = defineEmits<{ open: [] }>();
const { preference, select, saved } = useAppearance();
const options = computed(() => [
  { key: 'system', label: '跟随系统', icon: 'monitor', checked: preference.value === 'system' },
  { key: 'light', label: '浅色', icon: 'sun', checked: preference.value === 'light' },
  { key: 'dark', label: '深色', icon: 'moon', checked: preference.value === 'dark' },
]);
</script>
<template>
  <div class="appearance-menu">
    <button
      type="button"
      class="icon-button skin-center-entry"
      aria-label="换肤"
      title="换肤"
      :aria-pressed="props.centerOpen"
      :disabled="props.disabled"
      @click="emit('open')"
    >
      <Icon name="palette" />
    </button>
    <ActionMenu
      label="外观主题"
      icon="down"
      :items="options"
      @select="($event === 'system' || $event === 'light' || $event === 'dark') && select($event)"
    />
    <span v-if="!saved" class="appearance-warning" role="status">外观已应用，本次未能保存</span>
  </div>
</template>

<style scoped>
.skin-center-entry {
  width: 29px;
}
.skin-center-entry[aria-pressed='true'] {
  color: var(--accent);
}
.appearance-menu :deep(.menu-trigger) {
  width: 17px;
}
.appearance-menu :deep(.menu-trigger .icon) {
  width: 13px;
}
</style>
