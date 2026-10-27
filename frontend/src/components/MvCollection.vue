<script setup lang="ts">
import { ref } from 'vue';
import type { MvModel } from '../composables/useMv';
import { videoTime } from '../services/mv';
const props = defineProps<{ model: MvModel }>();
const emit = defineEmits<{ open: [id: string] }>();
const { page, loading, error, notice, query, busy } = props.model;
const keyword = ref(query.value.keyword),
  address = ref(''),
  inputError = ref('');
function openAddress() {
  inputError.value = '';
  const value = address.value.trim(),
    id = /^\d+$/.test(value) ? value : value.match(/(?:[?&]id=|\/mv\/)(\d+)/)?.[1];
  if (!id || id === '0') {
    inputError.value = '请输入 MV 编号或含 MV 编号的链接';
    return;
  }
  emit('open', id);
}
</script>
<template>
  <section class="mv-collection" aria-label="收藏的MV">
    <div class="page-heading">
      <div>
        <h1>收藏的 MV</h1>
        <p class="muted">{{ page?.total ?? 0 }} 个视频</p>
      </div>
      <button class="text-button" :disabled="loading" @click="model.load(true)">刷新</button>
    </div>
    <div class="mv-tools">
      <form @submit.prevent="model.filter(keyword)">
        <input
          v-model="keyword"
          aria-label="筛选收藏MV"
          placeholder="搜索收藏的 MV"
          maxlength="200"
        /><button class="text-button">筛选</button>
      </form>
      <form @submit.prevent="openAddress">
        <input v-model="address" aria-label="MV编号或链接" placeholder="MV 编号或链接" /><button
          class="text-button"
        >
          打开 MV
        </button>
      </form>
    </div>
    <p v-if="inputError" class="form-warning" role="alert">{{ inputError }}</p>
    <p v-if="error" class="error-state" role="alert">
      {{ error }} <button class="text-button" @click="model.load(true)">重试</button>
    </p>
    <p v-if="notice" class="notice" role="status">{{ notice }}</p>
    <div v-for="op in page?.operations" :key="op.id" class="notice">
      MV {{ op.id }} 的收藏操作待确认
      <button class="text-button" :disabled="busy" @click="model.collect(op.id, op.desired, true)">
        核对状态
      </button>
    </div>
    <div v-if="loading && !page" class="empty-state"><span class="spinner" />正在加载收藏…</div>
    <div v-else-if="page && !page.cards.length" class="empty-state">
      {{ query.keyword ? '没有匹配的 MV' : '还没有收藏 MV' }}
    </div>
    <div v-else class="mv-grid">
      <button
        v-for="card in page?.cards"
        :key="card.id"
        class="mv-card"
        @click="emit('open', card.id)"
      >
        <div class="mv-cover">
          <img
            v-if="card.coverUrl"
            :src="card.coverUrl"
            alt=""
            loading="lazy"
            referrerpolicy="no-referrer"
          /><span class="mv-count">▷ {{ card.playCount.toLocaleString() }}</span
          ><span class="mv-duration">{{ videoTime(card.durationMs / 1000) }}</span
          ><span class="mv-hover-play">▶</span>
        </div>
        <span class="mv-card-title">{{ card.title }}</span
        ><small>{{ card.artists.join(' / ') }}</small>
      </button>
    </div>
    <div v-if="page && page.total > query.limit" class="mv-pagination">
      <button
        class="text-button"
        :disabled="loading || !query.offset"
        @click="model.paginate(Math.max(0, query.offset - query.limit))"
      >
        上一页</button
      ><span
        >{{ query.offset + 1 }}–{{ Math.min(query.offset + query.limit, page.total) }} /
        {{ page.total }}</span
      ><button
        class="text-button"
        :disabled="loading || query.offset + query.limit >= page.total"
        @click="model.paginate(query.offset + query.limit)"
      >
        下一页
      </button>
    </div>
  </section>
</template>
