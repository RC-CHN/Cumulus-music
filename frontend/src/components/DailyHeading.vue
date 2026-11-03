<script setup lang="ts">
import { computed, shallowRef, watch } from 'vue';

const props = defineProps<{ birthday?: boolean; nickname?: string | null; compact?: boolean }>();
// The date is local and updates if a native read changes birthday presentation.
const today = shallowRef(new Date());
watch(
  () => props.birthday,
  () => {
    today.value = new Date();
  },
);
const day = computed(() => today.value.getDate());
const month = computed(() => today.value.getMonth() + 1);
const date = computed(
  () =>
    `${today.value.getFullYear()}-${String(month.value).padStart(2, '0')}-${String(day.value).padStart(2, '0')}`,
);
const title = computed(() => (props.birthday ? '生日特别推荐' : '每日推荐'));
const description = computed(() => {
  if (!props.birthday) return '根据你的音乐口味生成，每天 6:00 更新';
  const name = props.nickname?.trim();
  return name ? `亲爱的${name}，祝您生日快乐🎂` : '祝您生日快乐🎂';
});
</script>

<template>
  <div
    class="daily-heading-copy"
    :class="{ 'daily-birthday': birthday, 'daily-heading-compact': compact }"
  >
    <component :is="compact ? 'div' : 'h1'" class="daily-heading-title" :aria-label="title">
      <time :datetime="date" :aria-label="`${month}月${day}日`">
        <span class="daily-day">{{ day }}</span>
        <span class="daily-month"><span class="daily-date-divider">/</span>{{ month }}</span>
      </time>
      <span class="daily-title">{{ title }}</span>
    </component>
    <p v-if="!compact" class="daily-description">{{ description }}</p>
  </div>
</template>

<style scoped>
.daily-heading-copy {
  min-width: 0;
}
.daily-birthday {
  padding: 6px 14px 9px;
  border-radius: 12px;
  background: linear-gradient(
    100deg,
    color-mix(in srgb, var(--accent) 13%, var(--surface-alt)),
    transparent
  );
}
.daily-birthday .daily-description {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.daily-birthday .daily-title {
  color: var(--accent);
}

.daily-heading-title,
time {
  display: flex;
  align-items: center;
}
.daily-heading-title {
  height: 48px;
  letter-spacing: 0;
}
.daily-heading-compact {
  padding: 0;
  background: none;
}
.daily-heading-compact .daily-heading-title,
.daily-heading-compact .daily-day {
  height: auto;
}
.daily-heading-compact .daily-day,
.daily-heading-compact .daily-month,
.daily-heading-compact .daily-title {
  font-size: 13px;
  line-height: 20px;
}
h1:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 4px;
  border-radius: 4px;
}
time {
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
}
.daily-day {
  font-size: 40px;
  height: 48px;
  line-height: 40px;
}
.daily-month {
  font-size: 20px;
  height: 28px;
  line-height: 28px;
}
.daily-date-divider {
  margin: 0 6px;
}
.daily-title {
  font-size: 22px;
  font-weight: 600;
  margin-left: 8px;
  line-height: 32px;
  white-space: nowrap;
}
.daily-description {
  font-size: 13px;
  line-height: 21px;
  color: var(--muted);
  margin-top: 8px;
}
</style>
