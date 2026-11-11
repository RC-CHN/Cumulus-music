<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref } from 'vue';
import type { TrackView } from '../services/contracts';
import type { AlbumCard } from '../services/catalog';
import { albumLink, mvPublicLink, type CatalogMvShare } from '../services/catalogCards';
import { songLink } from '../services/trackMenu';
import { playlistLink, type PlaylistActionTarget } from '../services/playlistMenu';
import {
  podcastProgramPublicLink,
  podcastPublicLink,
  type PodcastProgramShare,
} from '../services/podcast';
import { useCore } from '../services/core';
import Icon from './Icon.vue';
const props = defineProps<{
  track?: TrackView;
  playlist?: PlaylistActionTarget;
  album?: AlbumCard;
  podcast?: { id: string; name: string };
  program?: PodcastProgramShare;
  mv?: CatalogMvShare;
  canCopy?: () => boolean;
  beforeCopy?: () => Promise<string | undefined>;
}>();
const kind = computed(() =>
  props.mv
    ? 'MV'
    : props.program
      ? '声音'
      : props.podcast
        ? '播客'
        : props.album
          ? '专辑'
          : props.playlist
            ? '歌单'
            : '歌曲',
);
const link = computed(() =>
  props.mv
    ? mvPublicLink(props.mv.id)
    : props.program
      ? podcastProgramPublicLink(props.program.id)
      : props.podcast
        ? podcastPublicLink(props.podcast.id)
        : props.album
          ? albumLink(props.album.id)
          : props.playlist
            ? playlistLink(props.playlist)
            : props.track
              ? songLink(props.track)
              : '',
);
const name = computed(
  () =>
    props.mv?.name ??
    props.program?.name ??
    props.podcast?.name ??
    props.album?.name ??
    props.playlist?.name ??
    props.track?.name ??
    '',
);
const subtitle = computed(
  () =>
    props.mv?.creator ??
    props.program?.creator ??
    props.album?.artists.map((artist) => artist.name).join(' / ') ??
    props.playlist?.creator ??
    props.track?.artists.join(' / '),
);
const emit = defineEmits<{ close: [] }>();
const dialog = ref<HTMLDialogElement>();
const client = useCore(),
  busy = ref(false),
  message = ref(''),
  failed = ref(false);
let alive = true;
onMounted(() => dialog.value?.showModal());
onBeforeUnmount(() => {
  alive = false;
});
async function copy() {
  if (!link.value || busy.value || props.canCopy?.() === false) return;
  busy.value = true;
  message.value = '';
  failed.value = false;
  try {
    if (props.beforeCopy) {
      const restriction = await props.beforeCopy();
      if (!alive) return;
      if (restriction) {
        message.value = restriction;
        failed.value = true;
        return;
      }
    }
    if (props.canCopy?.() === false) return;
    await client.copyText(link.value);
    if (alive) message.value = '链接已复制';
  } catch {
    if (alive) {
      message.value = '复制失败，请重试';
      failed.value = true;
    }
  } finally {
    if (alive) busy.value = false;
  }
}
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog track-share-dialog"
    aria-labelledby="track-share-title"
    @cancel.prevent="emit('close')"
  >
    <header>
      <h2 id="track-share-title">分享{{ kind }}</h2>
      <button class="icon-button" aria-label="关闭分享" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
    <div class="track-share-body">
      <strong>{{ name }}</strong>
      <p class="muted">{{ subtitle }}</p>
      <p>复制{{ kind }}链接，与朋友分享</p>
      <input
        v-if="!beforeCopy"
        :aria-label="`${kind}分享链接`"
        :value="link"
        readonly
        @focus="($event.target as HTMLInputElement).select()"
      />
      <button class="primary" :aria-disabled="busy" @click="copy">
        <Icon name="copy" />{{ busy ? '正在复制…' : '复制链接' }}
      </button>
      <p class="track-share-status" :class="{ failed }" role="status">{{ message }}</p>
    </div>
  </dialog>
</template>
