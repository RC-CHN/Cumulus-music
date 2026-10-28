<script setup lang="ts">
import { computed } from 'vue';
import ActionMenu from './ActionMenu.vue';
import { useAppearance } from '../composables/useAppearance';
import { appearancePreference } from '../services/appearance';
const { preference, select, saved } = useAppearance();
const options = computed(() => [
  { key: 'system', label: '跟随系统', icon: 'monitor', checked: preference.value === 'system' },
  { key: 'light', label: '浅色', icon: 'sun', checked: preference.value === 'light' },
  { key: 'dark', label: '深色', icon: 'moon', checked: preference.value === 'dark' },
]);
</script>
<template>
  <div class="appearance-menu">
    <ActionMenu
      label="外观主题"
      icon="palette"
      :items="options"
      @select="select(appearancePreference($event))"
    />
    <span v-if="!saved" class="appearance-warning" role="status">外观已应用，本次未能保存</span>
  </div>
</template>
