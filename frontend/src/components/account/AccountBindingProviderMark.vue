<script setup lang="ts">
import type { BindingMethod } from '../../services/accountBindings';
import Icon from '../Icon.vue';

defineProps<{ method: BindingMethod; bound?: boolean }>();
</script>

<template>
  <span class="provider-mark" :class="{ bound }" :data-provider="method" aria-hidden="true">
    <Icon v-if="method === 'cellphone'" name="phone" />
    <Icon v-else-if="method === 'email'" name="mail" />
    <Icon v-else-if="method === 'wechat'" name="chatPair" />
    <span v-else>{{ method === 'qq' ? 'QQ' : '微博' }}</span>
  </span>
</template>

<style scoped>
.provider-mark {
  display: grid;
  place-items: center;
  box-sizing: border-box;
  inline-size: var(--provider-mark-size, 40px);
  block-size: var(--provider-mark-size, 40px);
  flex: none;
  border: 1px solid var(--line);
  border-radius: 50%;
  background: var(--surface-alt);
  color: var(--text-secondary);
  font-size: var(--provider-mark-label, 12px);
  font-weight: 600;
}
.provider-mark > .icon {
  inline-size: var(--provider-mark-glyph, 23px);
  block-size: var(--provider-mark-glyph, 23px);
}
.provider-mark.bound {
  --provider-color: var(--accent);
  border-color: color-mix(in srgb, var(--provider-color) 20%, transparent);
  background: color-mix(in srgb, var(--provider-color) 9%, transparent);
  color: var(--provider-color);
}
.provider-mark.bound[data-provider='wechat'] {
  --provider-color: #2c8e2d;
}
.provider-mark.bound[data-provider='qq'] {
  --provider-color: #1f79bf;
}
</style>
