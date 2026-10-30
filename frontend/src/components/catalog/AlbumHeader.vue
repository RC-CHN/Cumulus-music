<script setup lang="ts">
import { computed, ref } from 'vue';
import type { AlbumCard, CatalogResource } from '../../services/catalog';
import ActionMenu from '../ActionMenu.vue';
import TrackShareDialog from '../TrackShareDialog.vue';
import ModalTransition from '../ModalTransition.vue';
import Icon from '../Icon.vue';
const props = defineProps<{
  album: AlbumCard;
  digital: boolean;
  subscribed: boolean | null;
  collectDisabled: boolean;
  playDisabled: boolean;
  downloadDisabled: boolean;
  refreshDisabled: boolean;
  refreshing: boolean;
}>();
const emit = defineEmits<{
  open: [resource: CatalogResource];
  play: [];
  collect: [];
  download: [];
  refresh: [];
}>();
const share = ref(false);
const actions = ref<HTMLDivElement>();
const shareReturnFocus = () =>
  actions.value?.querySelector<HTMLButtonElement>('.menu-trigger') ?? null;
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
  <header class="catalog-header album-header" :aria-busy="refreshing">
    <img
      v-if="album.coverUrl"
      class="catalog-cover"
      :src="album.coverUrl"
      alt=""
      referrerpolicy="no-referrer"
      decoding="async"
    />
    <div v-else class="catalog-cover catalog-placeholder"><Icon name="music" /></div>
    <div class="catalog-heading">
      <h1>
        {{ album.name
        }}<span v-if="album.aliases.length" class="album-alias"
          >（{{ album.aliases.join(' / ') }}）</span
        >
      </h1>
      <p v-if="album.translations.length" class="album-translation muted">
        {{ album.translations.join(' / ') }}
      </p>
      <div class="album-metadata">
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
        <button class="primary" :disabled="playDisabled" @click="emit('play')">
          <Icon name="play" />播放全部
        </button>
        <button
          :disabled="collectDisabled"
          :aria-pressed="subscribed ?? undefined"
          @click="emit('collect')"
        >
          <Icon name="star" />{{
            subscribed == null ? '收藏状态未知' : subscribed ? '已收藏' : '收藏'
          }}
        </button>
        <button :disabled="downloadDisabled" @click="emit('download')">
          <Icon name="download" />下载
        </button>
        <ActionMenu
          label="更多专辑操作"
          :icon="refreshing ? 'refresh' : 'more'"
          :class="{ 'catalog-refreshing': refreshing }"
          :items="[
            { key: 'share', label: '分享专辑', icon: 'share' },
            { key: 'refresh', label: '刷新详情', icon: 'refresh', disabled: refreshDisabled },
          ]"
          @select="$event === 'share' ? (share = true) : emit('refresh')"
        />
      </div>
    </div>
    <ModalTransition :return-focus="shareReturnFocus">
      <TrackShareDialog v-if="share" :album="album" @close="share = false" />
    </ModalTransition>
  </header>
</template>
