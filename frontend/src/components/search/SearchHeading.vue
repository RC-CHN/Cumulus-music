<script setup lang="ts">
defineProps<{ keyword: string; total?: number; loading: boolean; failed: boolean }>();
</script>
<template>
  <div class="search-heading-copy">
    <h1 :title="keyword" :aria-label="`搜索「${keyword}」`">{{ keyword }}</h1>
    <p>
      {{
        loading
          ? '正在寻找音乐…'
          : failed
            ? '加载失败，可以重试'
            : total !== undefined
              ? `找到 ${total} 首歌曲`
              : ''
      }}
    </p>
  </div>
</template>
<style scoped>
.search-heading-copy {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 0 8px;
  min-width: 0;
}
h1 {
  font-size: 22px;
  font-weight: 600;
  line-height: 32px;
  max-width: min(660px, 100%);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  letter-spacing: 0;
}
p {
  font-size: 13px;
  line-height: 16px;
  color: var(--muted);
  margin: 0 0 4px;
}
</style>
