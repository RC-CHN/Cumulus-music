<script setup lang="ts">
import BoundedPanelTransition from './BoundedPanelTransition.vue';
import { computed, nextTick, onBeforeUnmount, ref, useId, watch } from 'vue';
import type { AccountView } from '../services/contracts';
import type { CatalogResource } from '../services/catalog';
import type { AlbumCard } from '../services/catalog';
import type { AlbumCardRequest } from '../services/catalogCards';
import type { ArtistCardRequest } from '../services/artistCards';
import type { PlayerOrigin } from '../services/player';
import type { CollectionScope } from '../services/collections';
import { normalizeKeyword } from '../services/keyword';
import { useCollections } from '../composables/useCollections';
import { useCollectionViewport } from '../composables/useCollectionViewport';
import type { CollectionViewportMemory } from '../services/collectionViewport';
import CatalogCards from './catalog/CatalogCards.vue';
import FollowCards from './collections/FollowCards.vue';
import Icon from './Icon.vue';
const props = defineProps<{
  account: AccountView;
  scope: CollectionScope;
  visit: number;
  memory: CollectionViewportMemory;
  revision: number;
  playable: boolean;
  queueable: boolean;
  origin?: PlayerOrigin | null;
  playing: boolean;
}>();
const emit = defineEmits<{
  open: [resource: CatalogResource];
  scope: [scope: CollectionScope];
  expired: [];
  playAlbum: [album: AlbumCard];
  albumAction: [request: AlbumCardRequest];
  artistAction: [request: ArtistCardRequest];
}>();
const model = useCollections(undefined, () => emit('expired'));
const keyword = ref('');
const root = ref<HTMLElement>(),
  input = ref<HTMLInputElement>();
const panelId = useId();
let owner = props.account.epoch,
  visit = props.visit;
let scopeRevision = props.memory.revision(props.scope);
const owns = () =>
  props.account.status === 'authenticated' &&
  props.account.epoch === owner &&
  props.visit === visit &&
  !root.value?.closest('.page-leave-active');
const viewport = useCollectionViewport(
  root,
  () => !!model.pages.value.length || !!model.error.value,
  owns,
);
function checkpoint() {
  const data = model.snapshot();
  if (data) props.memory.set(visit, data, keyword.value, viewport.checkpoint(), scopeRevision);
}
defineExpose({ checkpoint });
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
let filterBaseline = needle.value;
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
  if (scope !== model.scope.value) emit('scope', scope);
}
function tabKey(event: KeyboardEvent, index: number) {
  const next =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? tabs.length - 1
        : event.key === 'ArrowRight'
          ? (index + 1) % tabs.length
          : event.key === 'ArrowLeft'
            ? (index + tabs.length - 1) % tabs.length
            : -1;
  if (next < 0) return;
  event.preventDefault();
  (event.currentTarget as HTMLElement).parentElement
    ?.querySelectorAll<HTMLButtonElement>('button')
    [next]?.focus({ preventScroll: true });
  choose(tabs[next]!.key);
}
async function clearKeyword() {
  keyword.value = '';
  await nextTick();
  if (owns() && input.value?.isConnected) input.value.focus({ preventScroll: true });
}
async function read(event: MouseEvent, action: () => Promise<void>) {
  const origin = event.currentTarget as HTMLButtonElement,
    epoch = owner,
    scope = model.scope.value,
    currentVisit = visit;
  const replace =
    action === model.refresh ||
    (action === model.retry && model.snapshot()?.retry?.append === false);
  await action();
  await nextTick();
  if (!owns() || epoch !== owner || scope !== model.scope.value || currentVisit !== visit) return;
  if (replace && !model.error.value) viewport.reset();
  if (document.activeElement === document.body)
    (origin.isConnected && !origin.disabled
      ? origin
      : root.value?.querySelector<HTMLButtonElement>('[data-collection-key="control:refresh"]')
    )?.focus({ preventScroll: true });
}
// Official PageFavAlbum continues searching later pages when the loaded page
// has no matching album. Stop on error; a retry is always an explicit action.
watch([needle, visibleCount, model.next, model.loading, model.error, root], () => {
  if (
    needle.value &&
    visibleCount.value === 0 &&
    model.next.value &&
    !model.loading.value &&
    !model.error.value &&
    root.value &&
    owns()
  )
    void model.more();
});
watch(
  [
    () => props.account.epoch,
    () => props.account.status,
    () => props.scope,
    () => props.visit,
    () => props.revision,
  ],
  () => {
    checkpoint();
    viewport.cancel();
    owner = props.account.epoch;
    visit = props.visit;
    scopeRevision = props.memory.revision(props.scope);
    model.connect(props.account);
    const saved = props.memory.get(visit, props.scope, owner);
    keyword.value = props.memory.keyword(visit, props.scope, owner);
    filterBaseline = needle.value;
    if (!saved || !model.restore(saved.data, props.scope)) {
      model.reset();
      void model.open(props.scope);
    } else if (!model.pages.value.length && !model.error.value) void model.open(props.scope);
    viewport.enter(saved?.position);
  },
  { immediate: true },
);
watch(needle, (value) => {
  if (value === filterBaseline) return;
  filterBaseline = value;
  viewport.reset();
});
watch([model.pages, model.error, root], () => void viewport.restore(), { flush: 'post' });
onBeforeUnmount(() => {
  checkpoint();
  model.dispose();
});
</script>
<template>
  <section
    ref="root"
    class="collections-page"
    aria-label="专辑与关注"
    :aria-busy="model.loading.value"
    @focusin="viewport.remember"
    @wheel.passive="viewport.cancel"
    @touchmove.passive="viewport.cancel"
    @pointerdown="viewport.cancel"
    @keydown="viewport.cancel"
  >
    <header class="collections-heading">
      <h1 tabindex="-1">专辑与关注</h1>
      <button
        class="icon-button"
        aria-label="刷新收藏列表"
        data-collection-key="control:refresh"
        :disabled="model.loading.value"
        @click="read($event, model.refresh)"
      >
        <Icon name="refresh" />
      </button>
    </header>
    <nav class="collection-tabs" aria-label="收藏类型" role="tablist">
      <button
        v-for="(tab, index) in tabs"
        :key="tab.key"
        :aria-current="model.scope.value === tab.key ? 'page' : undefined"
        role="tab"
        :id="`${panelId}-${tab.key}`"
        :aria-selected="model.scope.value === tab.key"
        :aria-controls="panelId"
        :tabindex="model.scope.value === tab.key ? 0 : -1"
        :data-collection-key="`control:${tab.key}`"
        @click="choose(tab.key)"
        @keydown="tabKey($event, index)"
      >
        {{ tab.label }}
      </button>
    </nav>
    <div class="collections-tools">
      <label class="collections-search"
        ><Icon name="search" /><input
          v-model="keyword"
          ref="input"
          data-collection-key="control:filter"
          aria-label="筛选收藏列表"
          placeholder="搜索名称或歌手"
        /><button v-if="keyword" class="icon-button" aria-label="清空筛选" @click="clearKeyword">
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
      ><button
        class="text-button"
        data-collection-key="control:retry"
        :disabled="model.loading.value"
        @click="read($event, model.retry)"
      >
        重试
      </button>
    </div>
    <p v-if="model.loading.value" role="status" class="muted">
      {{ keyword && !visibleCount ? '正在继续查找…' : '正在加载…' }}
    </p>
    <BoundedPanelTransition name="catalog-tab" @after-enter="viewport.restore">
      <div
        :key="model.scope.value"
        :id="panelId"
        class="collections-content"
        role="tabpanel"
        :aria-labelledby="`${panelId}-${model.scope.value}`"
      >
        <FollowCards
          v-if="model.scope.value === 'followed_artists' && visibleCount"
          :entries="filteredFollows"
          @artist="emit('open', { kind: 'artist', id: $event })"
          @action="emit('artistAction', $event)"
        />
        <CatalogCards
          v-else-if="model.scope.value !== 'followed_artists' && visibleCount"
          :albums="filteredAlbums"
          :playable="playable"
          :queueable="queueable"
          :origin="origin"
          :playing="playing"
          :saved="scope === 'saved_albums'"
          @open="emit('open', $event)"
          @play-album="emit('playAlbum', $event)"
          @album-action="emit('albumAction', $event)"
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
    </BoundedPanelTransition>
    <button
      v-if="model.next.value"
      class="load-more"
      data-collection-key="control:more"
      :disabled="model.loading.value"
      @click="read($event, model.more)"
    >
      {{ model.loadingMore.value ? '正在加载…' : '加载更多' }}
    </button>
  </section>
</template>
