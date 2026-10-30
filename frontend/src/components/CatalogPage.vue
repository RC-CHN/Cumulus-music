<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useId, watch } from 'vue';
import type { TrackMenuRequest } from '../services/trackMenu';
import type { AccountView, TrackView } from '../services/contracts';
import type { ArtistTab, CatalogResource, CatalogSelectionPage } from '../services/catalog';
import type { AlbumTab, CatalogPanelKey } from '../services/catalogNavigation';
import type { CatalogSession } from '../composables/useCatalogSession';
import TrackTable from './TrackTable.vue';
import CatalogCards from './catalog/CatalogCards.vue';
import AlbumHeader from './catalog/AlbumHeader.vue';
import AlbumContent from './catalog/AlbumContent.vue';
import Icon from './Icon.vue';
import ArtistHeader from './catalog/ArtistHeader.vue';
import ArtistTabs from './catalog/ArtistTabs.vue';
const props = defineProps<{
  model: CatalogSession;
  account: AccountView;
  playable: boolean;
  playBusy: boolean;
  downloadBusy: boolean;
  liked: Set<string>;
  busyLikes: Set<string>;
  currentId?: string;
}>();
const emit = defineEmits<{
  open: [resource: CatalogResource];
  play: [pages: CatalogSelectionPage[], index: number];
  like: [track: TrackView];
  more: [request: TrackMenuRequest];
  download: [tracks: TrackView[]];
  mv: [id: string];
  expired: [];
  artistTab: [tab: ArtistTab];
  albumTab: [tab: AlbumTab];
  back: [];
}>();
const {
  resource,
  tab,
  albumTab,
  positions,
  detail,
  dynamic,
  membership,
  list,
  artist,
  album,
  hotTracks,
  onlineCount,
  subscribed,
  stateReading,
  tracks,
  selections,
  next,
  intro,
  auxiliary,
  cards,
  refreshState,
  refresh,
  collect,
} = props.model;
const expire = () => emit('expired');
const section = ref<HTMLElement>(),
  panel = ref<HTMLElement>();
const panelId = useId();
const selectedPanel = computed<CatalogPanelKey>(() =>
  resource.kind === 'artist' ? `artist:${tab.value}` : `album:${albumTab.value}`,
);
let pendingRestore: CatalogPanelKey | undefined = positions.has(selectedPanel.value)
  ? selectedPanel.value
  : undefined;
let restoring = 0;
function panelKey(element: Element): CatalogPanelKey | undefined {
  const node = element as HTMLElement;
  return node.dataset.artistPanel
    ? `artist:${node.dataset.artistPanel as ArtistTab}`
    : node.dataset.albumPanel
      ? `album:${node.dataset.albumPanel as AlbumTab}`
      : undefined;
}
function currentPanel(key = selectedPanel.value) {
  return resource.kind === 'artist'
    ? panel.value
    : section.value?.querySelector<HTMLElement>(`[data-album-panel="${key.slice(6)}"]`);
}
function checkpoint(key = selectedPanel.value) {
  if (!props.model.valid()) return;
  const node = currentPanel(key);
  if (!node || panelKey(node) !== key || pendingRestore === key) return;
  const inner = node.querySelector<HTMLElement>('.track-scroll,.catalog-card-scroll');
  const active = document.activeElement;
  const focus =
    active instanceof HTMLElement && section.value?.contains(active)
      ? active.closest<HTMLElement>('[data-catalog-key]')?.dataset.catalogKey
      : positions.get(key)?.focus;
  positions.set(key, { inner: inner?.scrollTop ?? 0, outer: section.value?.scrollTop ?? 0, focus });
}
function cancelRestore(event?: Event) {
  if (event instanceof KeyboardEvent && (event.target as Element)?.closest('.catalog-tabs')) return;
  pendingRestore = undefined;
  restoring++;
}
function restore(element: Element) {
  const owner = selectedPanel.value,
    saved = positions.get(owner),
    ticket = ++restoring;
  if (pendingRestore !== owner || !saved || panelKey(element) !== owner) return;
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      if (
        !props.model.valid() ||
        ticket !== restoring ||
        pendingRestore !== owner ||
        selectedPanel.value !== owner ||
        !element.isConnected ||
        panelKey(element) !== owner
      )
        return;
      const inner = element.querySelector<HTMLElement>('.track-scroll,.catalog-card-scroll');
      if (inner) {
        inner.scrollTop = saved.inner;
        inner.dispatchEvent(new Event('scroll'));
      }
      if (section.value) section.value.scrollTop = saved.outer;
      const returnTarget = document.activeElement;
      if (
        saved.focus &&
        (returnTarget === document.body || returnTarget?.classList.contains('back-button'))
      )
        requestAnimationFrame(() => {
          // The scroll event updates the virtual range in this frame. Wait for
          // Vue to commit those rows before looking up the original card.
          void nextTick(() => {
            if (
              props.model.valid() &&
              ticket === restoring &&
              element.isConnected &&
              document.activeElement === returnTarget
            )
              element
                .querySelector<HTMLElement>(`[data-catalog-key="${CSS.escape(saved.focus!)}"]`)
                ?.focus({ preventScroll: true });
          });
        });
      pendingRestore = undefined;
    }),
  );
}
watch(
  selectedPanel,
  (next, previous) => {
    checkpoint(previous);
    restoring++;
    pendingRestore = positions.has(next) ? next : undefined;
  },
  { flush: 'sync' },
);
onMounted(() => {
  void nextTick(() => {
    const node = currentPanel();
    if (node) restore(node);
  });
});
onBeforeUnmount(() => {
  checkpoint();
  cancelRestore();
});
function choose(next: ArtistTab) {
  if (next !== tab.value) emit('artistTab', next);
}
function playHot() {
  const index = hotTracks.value.findIndex((track) => !track.missing);
  if (index >= 0 && detail.view.value)
    emit('play', [{ query: detail.view.value.query, revision: detail.view.value.revision }], index);
}
function play(index: number) {
  if (tracks.value[index] && !tracks.value[index].missing) emit('play', selections.value, index);
}
defineExpose({ checkpoint: () => checkpoint(), resourceKey: `${resource.kind}:${resource.id}` });
</script>
<template>
  <section
    ref="section"
    class="catalog-page"
    @wheel.passive="cancelRestore"
    @touchstart.passive="cancelRestore"
    @pointerdown="cancelRestore"
    @keydown="cancelRestore"
    :aria-label="`${resource.kind === 'artist' ? '歌手' : '专辑'}详情`"
  >
    <div
      v-if="detail.error.value || list.error.value"
      class="catalog-error catalog-detail-error"
      role="alert"
    >
      <div v-if="detail.error.value">
        {{ detail.error.value }}<span v-if="detail.view.value"> · 保留上次内容</span
        ><button class="text-button" :disabled="detail.loading.value" @click="refresh">重试</button>
      </div>
      <div v-if="list.error.value" class="catalog-list-error">
        {{ list.error.value }}<span v-if="list.pages.value.length"> · 保留已加载内容</span
        ><button class="text-button" :disabled="list.loading.value" @click="list.retry">
          重试
        </button>
      </div>
    </div>
    <div v-if="!detail.view.value" class="empty-state" role="status">
      <span v-if="detail.loading.value" class="spinner" />{{
        detail.loading.value ? '正在加载详情…' : '暂无可显示的详情'
      }}
    </div>
    <template v-else>
      <AlbumHeader
        v-if="album"
        :album="album"
        :digital="auxiliary?.digitalAlbumTag === true"
        :subscribed="subscribed ?? null"
        :collect-disabled="
          subscribed == null ||
          stateReading ||
          !membership.known.value ||
          membership.busy.value ||
          membership.pending.value
        "
        :play-disabled="!playable || playBusy || !tracks.some((t) => !t.missing)"
        :download-disabled="downloadBusy || !tracks.some((t) => !t.missing)"
        :refresh-disabled="detail.loading.value || membership.busy.value"
        :refreshing="detail.loading.value"
        @open="emit('open', $event)"
        @play="play(tracks.findIndex((t) => !t.missing))"
        @collect="collect()"
        @download="
          emit(
            'download',
            tracks.filter((t) => !t.missing),
          )
        "
        @refresh="refresh"
      />
      <ArtistHeader
        v-else-if="artist && tab !== 'all'"
        :artist="artist"
        :followed="subscribed ?? null"
        :follow-disabled="
          subscribed == null ||
          stateReading ||
          !membership.known.value ||
          membership.busy.value ||
          membership.pending.value
        "
        :play-disabled="!playable || playBusy || !hotTracks.some((track) => !track.missing)"
        :refresh-disabled="
          detail.loading.value ||
          dynamic.loading.value ||
          list.loading.value ||
          membership.busy.value
        "
        :refreshing="detail.loading.value || dynamic.loading.value"
        @play="playHot"
        @follow="collect()"
        @refresh="refresh"
      />
      <p
        v-if="detail.loading.value || dynamic.loading.value"
        class="sr-only"
        role="status"
        data-catalog-updating
      >
        正在更新详情…
      </p>
      <div v-if="dynamic.error.value || membership.error.value" class="catalog-error" role="alert">
        {{ dynamic.error.value || membership.error.value
        }}<button class="text-button" :disabled="membership.busy.value" @click="refreshState">
          重读{{ artist ? '关注' : '收藏' }}状态
        </button>
      </div>
      <div v-if="membership.pending.value" class="notice" role="status">
        上次{{ artist ? '关注' : '收藏' }}操作结果待确认<button
          class="text-button"
          :disabled="membership.busy.value"
          @click="collect(true)"
        >
          核对结果
        </button>
      </div>
      <p v-if="auxiliary && !auxiliary.privilegesRefreshed" class="notice">
        歌曲权益暂未更新，播放时将重新检查。
      </p>
      <ArtistTabs
        v-if="artist && tab !== 'all'"
        :artist="artist"
        :selected="tab"
        :online-count="onlineCount"
        :id-prefix="panelId"
        @choose="choose"
      />
      <AlbumContent
        v-if="album"
        :key="`${account.epoch}:${album.id}`"
        :album="album"
        :model="model"
        :account="account"
        :tracks="tracks"
        :liked="liked"
        :busy-likes="busyLikes"
        :playable="playable && !playBusy"
        :current-id="currentId"
        @choose="emit('albumTab', $event)"
        @enter="restore"
        @play="play"
        @like="emit('like', $event)"
        @more="emit('more', $event)"
        @open="emit('open', $event)"
        @expired="expire"
      />
      <Transition
        v-else
        name="catalog-tab"
        mode="out-in"
        @enter="restore"
        @before-leave="($event as HTMLElement).inert = true"
        @before-enter="($event as HTMLElement).inert = false"
      >
        <div
          :id="`${panelId}-panel`"
          ref="panel"
          :data-artist-panel="tab"
          :key="tab"
          class="catalog-tab-panel"
          :role="tab === 'all' ? undefined : 'tabpanel'"
          :aria-labelledby="tab === 'all' ? undefined : `${panelId}-${tab}`"
        >
          <template v-if="tab === 'songs' || tab === 'all'">
            <div class="catalog-section-heading" v-if="artist">
              <h1 v-if="tab === 'all'" class="artist-all-title">
                全部歌曲 <small>{{ artist?.name }}</small>
              </h1>
              <h2 v-else>热门歌曲 TOP 50</h2>
              <button v-if="tab === 'songs'" class="text-button" @click="choose('all')">
                查看全部歌曲 →</button
              ><button v-else class="text-button" @click="emit('back')">返回热门歌曲</button>
            </div>
            <div class="catalog-actions catalog-track-actions">
              <button
                v-if="tab === 'all'"
                class="primary"
                :disabled="!playable || playBusy || !tracks.some((t) => !t.missing)"
                @click="play(tracks.findIndex((t) => !t.missing))"
              >
                <Icon name="play" />{{ tab === 'all' ? '播放已加载歌曲' : '播放全部' }}
              </button>
              <button
                :disabled="downloadBusy || !tracks.some((t) => !t.missing)"
                @click="
                  emit(
                    'download',
                    tracks.filter((t) => !t.missing),
                  )
                "
              >
                下载当前歌曲列表
              </button>
              <span class="muted"
                >{{ tracks.length }} 首<span v-if="tab === 'all' && artist">
                  / {{ artist.songCount }} 首</span
                ></span
              >
            </div>
            <TrackTable
              v-if="tracks.length"
              navigable
              :tracks="tracks"
              :liked="liked"
              :busy="busyLikes"
              :playable="playable && !playBusy"
              :current-id="currentId"
              @play="play"
              @like="emit('like', $event)"
              @more="emit('more', $event)"
              @artist="emit('open', { kind: 'artist', id: $event })"
              @album="emit('open', { kind: 'album', id: $event })"
            />
            <p v-else-if="!list.loading.value && !list.error.value" class="empty-state">暂无歌曲</p>
          </template>
          <template v-else-if="tab === 'intro'">
            <article v-if="intro" class="catalog-description">
              <p>{{ intro.briefDescription }}</p>
              <section v-for="(section, i) in intro.sections" :key="i">
                <h2>{{ section.title }}</h2>
                <p>{{ section.text }}</p>
              </section>
              <p v-if="!intro.briefDescription && !intro.sections.length">暂无歌手介绍</p>
            </article>
          </template>
          <CatalogCards
            v-else-if="list.pages.value.length"
            v-bind="cards"
            @open="emit('open', $event)"
            @mv="emit('mv', $event)"
          />
          <p v-if="list.loading.value" class="catalog-loading" role="status">正在加载…</p>
          <button v-if="next" class="load-more" :disabled="list.loading.value" @click="list.more">
            {{ list.loadingMore.value ? '正在加载…' : '加载更多' }}
          </button>
        </div>
      </Transition>
    </template>
  </section>
</template>
