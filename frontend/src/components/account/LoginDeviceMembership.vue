<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { LoginDeviceLimits } from '../../services/accountDetail';
import Icon from '../Icon.vue';

const props = defineProps<{ limits: LoginDeviceLimits }>();
const avatarFailed = ref(false);
watch(
  () => props.limits.avatarUrl,
  () => (avatarFailed.value = false),
);
const tiers = computed(() => [
  { id: 'ordinary', label: '非会员/畅听', limit: props.limits.ordinaryLimit },
  { id: 'vip', label: '黑胶VIP', limit: props.limits.vipLimit },
  { id: 'svip', label: '黑胶SVIP', limit: props.limits.svipLimit },
]);
</script>

<template>
  <section class="login-device-membership" aria-label="登录设备额度">
    <div class="login-device-membership-identity">
      <span class="login-device-membership-avatar" aria-hidden="true">
        <img
          v-if="limits.avatarUrl && !avatarFailed"
          :src="limits.avatarUrl"
          alt=""
          decoding="async"
          referrerpolicy="no-referrer"
          @error="avatarFailed = true"
        />
        <Icon v-else name="user" />
      </span>
      <div>
        <div class="login-device-membership-name">
          <strong>{{ limits.nickname || '未知' }}</strong>
          <span v-if="limits.currentTier === 'vip'" class="login-device-membership-badge">VIP</span>
          <span v-if="limits.currentTier === 'svip'" class="login-device-membership-badge"
            >SVIP</span
          >
        </div>
        <p>已登录{{ limits.currentNum ?? '未知' }}台手机/电脑/平板设备</p>
      </div>
    </div>
    <table class="login-device-limit" aria-label="各会员档位设备额度">
      <thead>
        <tr>
          <th
            v-for="tier in tiers"
            :key="tier.id"
            scope="col"
            :data-current-tier="limits.currentTier === tier.id ? tier.id : undefined"
          >
            {{ tier.label }}
          </th>
        </tr>
      </thead>
      <tbody>
        <tr>
          <td
            v-for="tier in tiers"
            :key="tier.id"
            :data-current-tier="limits.currentTier === tier.id ? tier.id : undefined"
          >
            <span v-if="limits.currentTier === tier.id" class="login-device-limit-current"
              >当前</span
            >
            <span class="login-device-limit-value">{{ tier.limit }}<small>台</small></span>
          </td>
        </tr>
      </tbody>
    </table>
  </section>
</template>
