<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import type { AccountView } from '../services/contracts';
import type { CatalogResource } from '../services/catalog';
import type { CollectionScope } from '../services/collections';
import { normalizeKeyword } from '../services/keyword';
import { useCollections } from '../composables/useCollections';
import CatalogCards from './catalog/CatalogCards.vue';
import FollowCards from './collections/FollowCards.vue';
import Icon from './Icon.vue';
const props = defineProps<{ account: AccountView; scope: CollectionScope }>();
const emit = defineEmits<{
  open: [resource: CatalogResource];
  scope: [scope: CollectionScope];
  expired: [];
}>();
const model = useCollections(undefined, () => emit('expired'));
const keyword = ref('');
const tabs: { key: CollectionScope; label: string }[] = [
  { key: 'saved_albums', label: '收藏专辑' },
  { key: 'purchased_albums', label: '已购专辑' },
  { key: 'followed_artists', label: '关注歌手' },
];
const albums = computed(() =>
  model.pages.value.flatMap((v) => (v.page.kind === 'albums' ? v.page.albums : [])),
);
const follows = computed(() =>
  model.pages.value.flatMap((v) => (v.page.kind === 'follows' ? v.page.entries : [])),
);
const needle = computed(() => normalizeKeyword(keyword.value));
const filteredAlbums = computed(() =>
  albums.value.filter(
    (album) =>
      !needle.value ||
      [album.name, album.primaryArtist?.name ?? '', album.artists.map((a) => a.name).join('')].some(
        (s) => normalizeKeyword(s).includes(needle.value),
      ),
  ),
);
// Keyword filtering on followed artists is a community UI addition.
const filteredFollows = computed(() =>
  follows.value.filter(
    (entry) =>
      !needle.value ||
      [entry.artist?.name ?? '', entry.user?.name ?? ''].some((s) =>
        normalizeKeyword(s).includes(needle.value),
      ),
  ),
);
const visibleCount = computed(() =>
  model.scope.value === 'followed_artists'
    ? filteredFollows.value.length
    : filteredAlbums.value.length,
);
const loadedCount = computed(() => albums.value.length + follows.value.length);
function choose(scope: CollectionScope) {
  if (scope !== model.scope.value) keyword.value = '';
  emit('scope', scope);
}
// Official PageFavAlbum continues searching later pages when the loaded page
// has no matching album. Stop on error; a retry is always an explicit action.
watch([needle, visibleCount, model.next, model.loading, model.error], () => {
  if (
    needle.value &&
    visibleCount.value === 0 &&
    model.next.value &&
    !model.loading.value &&
    !model.error.value
  )
    void model.more();
});
watch(
  () => [props.account.epoch, props.account.status, props.scope],
  () => {
    keyword.value = '';
    model.connect(props.account);
    void model.open(props.scope);
  },
  { immediate: true },
);
onUnmounted(model.dispose);
</script>
<template>
  <section class="collections-page" aria-label="专辑与关注" :aria-busy="model.loading.value">
    <header class="collections-heading">
      <h1>专辑与关注</h1>
      <button
        class="icon-button"
        aria-label="刷新收藏列表"
        :disabled="model.loading.value"
        @click="model.refresh"
      >
        <Icon name="refresh" />
      </button>
    </header>
    <nav class="catalog-tabs" aria-label="收藏类型">
      <button
        v-for="tab in tabs"
        :key="tab.key"
        :aria-current="model.scope.value === tab.key ? 'page' : undefined"
        @click="choose(tab.key)"
      >
        {{ tab.label }}
      </button>
    </nav>
    <div class="collections-tools">
      <label class="collections-search"
        ><Icon name="search" /><input
          v-model="keyword"
          aria-label="筛选收藏列表"
          placeholder="搜索名称或歌手"
        /><button v-if="keyword" class="icon-button" aria-label="清空筛选" @click="keyword = ''">
          ×
        </button></label
      >
      <span class="muted" aria-live="polite"
        >已加载 {{ loadedCount
        }}<template v-if="model.total.value !== null"> / {{ model.total.value }}</template
        ><template v-if="keyword"> · 匹配 {{ visibleCount }}</template></span
      >
    </div>
    <div v-if="model.error.value" class="catalog-error" role="alert">
      {{ model.error.value }}<span v-if="loadedCount"> · 保留已加载内容</span
      ><button class="text-button" :disabled="model.loading.value" @click="model.retry">
        重试
      </button>
    </div>
    <p v-if="model.loading.value" role="status" class="muted">
      {{ keyword && !visibleCount ? '正在继续查找…' : '正在加载…' }}
    </p>
    <Transition name="catalog-tab" mode="out-in">
      <div :key="model.scope.value" class="collections-content">
        <FollowCards
          v-if="model.scope.value === 'followed_artists' && visibleCount"
          :entries="filteredFollows"
          @artist="emit('open', { kind: 'artist', id: $event })"
        />
        <CatalogCards
          v-else-if="model.scope.value !== 'followed_artists' && visibleCount"
          :albums="filteredAlbums"
          @open="emit('open', $event)"
        />
        <p
          v-else-if="!model.loading.value && !model.error.value && model.pages.value.length"
          class="empty-state"
        >
          {{
            keyword
              ? '没有匹配的内容'
              : model.scope.value === 'purchased_albums'
                ? '还没有已购专辑'
                : model.scope.value === 'followed_artists'
                  ? '还没有关注歌手'
                  : '还没有收藏专辑'
          }}
        </p>
      </div>
    </Transition>
    <button
      v-if="model.next.value"
      class="load-more"
      :disabled="model.loading.value"
      @click="model.more"
    >
      {{ model.loadingMore.value ? '正在加载…' : '加载更多' }}
    </button>
  </section>
</template>
