<script setup lang="ts">
import { computed, ref } from 'vue';
import type { AlbumCard, CatalogResource } from '../../services/catalog';
import ActionMenu from '../ActionMenu.vue';
import TrackShareDialog from '../TrackShareDialog.vue';
import ModalTransition from '../ModalTransition.vue';
import Icon from '../Icon.vue';
const props = defineProps<{
  album: AlbumCard;
  compact?: boolean;
  digital: boolean;
  subscribed: boolean | null;
  collectDisabled: boolean;
  playDisabled: boolean;
  downloadDisabled: boolean;
  batchDisabled: boolean;
  queueDisabled: boolean;
  refreshDisabled: boolean;
  refreshing: boolean;
}>();
const emit = defineEmits<{
  open: [resource: CatalogResource];
  play: [];
  collect: [];
  download: [];
  batch: [origin: HTMLElement | null];
  queue: [origin: HTMLElement | null];
  refresh: [];
}>();
const share = ref(false);
const actions = ref<HTMLDivElement>();
const shareReturnFocus = () =>
  actions.value?.querySelector<HTMLButtonElement>('.menu-trigger') ?? null;
function chooseAction(key: string) {
  if (key === 'share') share.value = true;
  if (key === 'batch') emit('batch', shareReturnFocus());
  if (key === 'queue') emit('queue', shareReturnFocus());
  if (key === 'refresh') emit('refresh');
}
const credits = computed(() =>
  props.album.artists.length
    ? props.album.artists
    : props.album.primaryArtist
      ? [props.album.primaryArtist]
      : [],
);
const published = computed(() => {
  if (props.album.publishedAt == null) return '';
  const date = new Date(props.album.publishedAt);
  if (!Number.isFinite(date.getTime())) return '';
  return [date.getFullYear(), date.getMonth() + 1, date.getDate()]
    .map((value) => String(value).padStart(2, '0'))
    .join('.');
});
</script>
<template>
  <header
    class="catalog-header album-header"
    :class="{ 'album-header-compact': compact }"
    :aria-label="compact ? '专辑快捷操作' : undefined"
    :aria-busy="refreshing"
  >
    <img
      v-if="!compact && album.coverUrl"
      class="catalog-cover"
      :src="album.coverUrl"
      alt=""
      referrerpolicy="no-referrer"
      decoding="async"
    />
    <div v-else-if="!compact" class="catalog-cover catalog-placeholder"><Icon name="music" /></div>
    <div class="catalog-heading">
      <h2 v-if="compact" class="ellipsis" :title="album.name">
        {{ album.name }}<span v-if="digital" class="album-digital">数字专辑</span>
      </h2>
      <h1 v-else>
        {{ album.name
        }}<span v-if="album.aliases.length" class="album-alias"
          >（{{ album.aliases.join(' / ') }}）</span
        >
      </h1>
      <p v-if="!compact && album.translations.length" class="album-translation muted">
        {{ album.translations.join(' / ') }}
      </p>
      <div v-if="!compact" class="album-metadata">
        <p class="catalog-credits">
          <template v-for="(credit, index) in credits" :key="index">
            <span v-if="index"> / </span>
            <button
              v-if="credit.id"
              class="text-button"
              @click="emit('open', { kind: 'artist', id: credit.id })"
            >
              {{ credit.name }}
            </button>
            <span v-else>{{ credit.name }}</span>
          </template>
          <span v-if="!credits.length" class="muted">歌手信息暂缺</span>
        </p>
        <p v-if="published" class="muted">{{ published }} 发布</p>
        <span v-if="digital" class="album-digital">数字专辑</span>
        <span v-if="album.explicit" class="album-explicit" aria-label="含成人内容">E</span>
      </div>
      <div ref="actions" class="catalog-actions album-actions">
        <button
          class="primary"
          data-album-play
          :aria-label="compact ? '播放全部' : undefined"
          :title="compact ? '播放全部' : undefined"
          :disabled="playDisabled"
          @click="emit('play')"
        >
          <Icon name="play" /><span v-if="!compact">播放全部</span>
        </button>
        <button
          :disabled="collectDisabled"
          :aria-pressed="subscribed ?? undefined"
          :aria-label="compact ? (subscribed ? '取消收藏' : '收藏') : undefined"
          :title="compact ? (subscribed ? '取消收藏' : '收藏') : undefined"
          @click="emit('collect')"
        >
          <Icon name="star" /><span v-if="!compact">{{
            subscribed == null ? '收藏状态未知' : subscribed ? '已收藏' : '收藏'
          }}</span>
        </button>
        <button
          :aria-label="compact ? '下载' : undefined"
          :title="compact ? '下载' : undefined"
          :disabled="downloadDisabled"
          @click="emit('download')"
        >
          <Icon name="download" /><span v-if="!compact">下载</span>
        </button>
        <ActionMenu
          label="更多专辑操作"
          :icon="refreshing ? 'refresh' : 'more'"
          :class="{ 'catalog-refreshing': refreshing }"
          :items="[
            { key: 'share', label: '分享专辑', icon: 'share' },
            { key: 'batch', label: '批量操作', icon: 'list', disabled: batchDisabled },
            {
              key: 'queue',
              label: '添加全部至播放列表',
              icon: 'plus',
              disabled: queueDisabled || batchDisabled,
            },
            { key: 'refresh', label: '刷新详情', icon: 'refresh', disabled: refreshDisabled },
          ]"
          @select="chooseAction"
        />
      </div>
    </div>
    <ModalTransition :return-focus="shareReturnFocus">
      <TrackShareDialog v-if="share" :album="album" @close="share = false" />
    </ModalTransition>
  </header>
</template>
