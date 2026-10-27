<script setup lang="ts">
import { computed, ref } from 'vue';
import type { FmModel } from '../composables/useFm';
import type { FmCard } from '../services/fm';
import Icon from './Icon.vue';
const props = defineProps<{ model: FmModel }>();
const emit = defineEmits<{ select: [card: FmCard, play: boolean] }>();
const { home, page, query, loading, error } = props.model.discovery;
const expanded = ref(false);
const zones = computed(() =>
  expanded.value ? home.value?.page.zones : home.value?.page.zones.slice(0, 5),
);
function zone(pageCode: string) {
  void props.model.discovery.load({ kind: 'zone', page_code: pageCode, tab_key: 'default' });
}
function tab(key: string) {
  if (query.value.kind === 'zone')
    void props.model.discovery.load({ ...query.value, tab_key: key });
}
</script>
<template>
  <section class="fm-explore" aria-label="选择漫游模式">
    <nav class="fm-zone-tabs" aria-label="漫游专区">
      <button :aria-pressed="query.kind === 'home'" @click="model.discovery.load({ kind: 'home' })">
        为你推荐
      </button>
      <button
        v-for="z in zones"
        :key="z.pageCode"
        :data-zone="z.pageCode"
        :aria-pressed="query.kind === 'zone' && query.page_code === z.pageCode"
        @click="zone(z.pageCode)"
      >
        {{ z.name }}
      </button>
      <button
        v-if="(home?.page.zones.length ?? 0) > 5"
        :aria-expanded="expanded"
        @click="expanded = !expanded"
      >
        {{ expanded ? '收起专区' : '更多专区' }}
      </button>
    </nav>
    <div v-if="error" class="error-state" role="alert">
      {{ error }}<button @click="model.discovery.load()">重试</button>
    </div>
    <nav
      v-if="(page?.page.tabs.filter((t) => t.name).length ?? 0) > 1"
      class="fm-zone-tabs"
      aria-label="专区分类"
    >
      <button
        v-for="t in page?.page.tabs.filter((t) => t.name)"
        :key="t.key"
        :aria-pressed="page?.page.activeTab === t.key"
        @click="tab(t.key)"
      >
        {{ t.name }}
      </button>
    </nav>
    <p v-if="loading" role="status">正在加载漫游推荐…</p>
    <p v-if="page?.page.defaultFallback" role="status">当前分类暂不可用，已显示专区默认推荐</p>
    <p v-if="!loading && page && !page.page.cards.length" class="empty-state">
      这个专区暂时没有漫游推荐
    </p>
    <div class="fm-card-grid">
      <div
        v-for="(card, i) in page?.page.cards"
        :key="JSON.stringify(card.source)"
        class="fm-source-card"
      >
        <div class="fm-card-cover">
          <button
            class="fm-card-preview"
            :data-fm-kind="card.source.kind"
            :data-fm-card="i"
            :disabled="model.busy.value"
            :aria-label="`查看${card.title}`"
            @click="emit('select', card, false)"
          >
            <img
              v-if="card.coverUrl"
              :src="card.coverUrl"
              alt=""
              referrerpolicy="no-referrer"
            /><Icon v-else name="music" /><strong>{{ card.title }}</strong></button
          ><button
            class="fm-card-play"
            :data-fm-play="card.source.kind"
            :aria-label="`播放${card.title}`"
            :disabled="model.busy.value"
            @click="emit('select', card, true)"
          >
            <Icon name="play" />
          </button>
        </div>
        <span class="fm-card-subtitle">{{ card.subtitle }}</span>
      </div>
    </div>
  </section>
</template>
<style scoped>
.fm-explore {
  margin: 18px 0 24px;
}
.fm-zone-tabs {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  margin-bottom: 14px;
}
.fm-zone-tabs button {
  border: 0;
  border-radius: 16px;
  padding: 7px 13px;
  background: var(--surface);
  color: var(--text-secondary);
  cursor: pointer;
}
.fm-zone-tabs button[aria-pressed='true'] {
  background: #fae9ed;
  color: #ec4141;
}
.fm-card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
  gap: 18px;
}
.fm-source-card {
  border: 0;
  padding: 0;
  text-align: left;
  color: inherit;
  background: none;
  cursor: pointer;
  min-width: 0;
}
.fm-card-cover {
  position: relative;
  display: block;
  aspect-ratio: 1;
  border-radius: 10px;
  overflow: hidden;
  background: linear-gradient(140deg, #69669a, #b8a5ad);
}
.fm-card-preview {
  border: 0;
  padding: 0;
  display: block;
  width: 100%;
  height: 100%;
  cursor: pointer;
  background: none;
  color: inherit;
}
.fm-card-preview > img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.fm-card-preview > strong {
  position: absolute;
  inset: 0 0 auto;
  padding: 16px 12px 32px;
  color: white;
  font-size: 18px;
  background: linear-gradient(#0008, transparent);
}
.fm-card-preview > .icon {
  width: 42px;
  height: 42px;
  position: absolute;
  top: 40%;
  left: 35%;
  color: white;
}
.fm-card-play {
  border: 0;
  cursor: pointer;
  position: absolute;
  bottom: 10px;
  right: 10px;
  border-radius: 50%;
  padding: 8px;
  background: #fff;
  color: #ec4141;
  display: flex;
}
.fm-card-subtitle {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  line-height: 1.6;
  margin-top: 8px;
  font-size: 13px;
}
.fm-source-card button:focus-visible {
  outline: 2px solid #ec4141;
  outline-offset: 4px;
}
</style>
