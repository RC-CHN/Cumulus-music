<script setup lang="ts">
// Official dailyRecommend/views/index.js uses the local calendar date on entry.
const today = new Date();
const day = today.getDate();
const month = today.getMonth() + 1;
const date = `${today.getFullYear()}-${String(month).padStart(2, '0')}-${String(day).padStart(2, '0')}`;
</script>

<template>
  <div class="daily-heading-copy">
    <h1 aria-label="每日推荐">
      <time :datetime="date" :aria-label="`${month}月${day}日`">
        <span class="daily-day">{{ day }}</span>
        <span class="daily-month"><span class="daily-date-divider">/</span>{{ month }}</span>
      </time>
      <span class="daily-title">每日推荐</span>
    </h1>
    <p class="daily-description">根据你的音乐口味生成，每天 6:00 更新</p>
  </div>
</template>

<style scoped>
.daily-heading-copy {
  min-width: 0;
}
h1,
time {
  display: flex;
  align-items: center;
}
h1 {
  height: 48px;
  letter-spacing: 0;
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
