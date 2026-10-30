<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref } from 'vue';
import type { TrackView, PlaylistView } from '../services/contracts';
import { songLink } from '../services/trackMenu';
import { playlistLink } from '../services/playlistMenu';
import { useCore } from '../services/core';
import Icon from './Icon.vue';
const props = defineProps<{ track?: TrackView; playlist?: PlaylistView }>();
const kind = computed(() => (props.playlist ? '歌单' : '歌曲'));
const link = computed(() =>
  props.playlist ? playlistLink(props.playlist) : props.track ? songLink(props.track) : '',
);
const name = computed(() => props.playlist?.name ?? props.track?.name ?? '');
const subtitle = computed(() => props.playlist?.creator ?? props.track?.artists.join(' / '));
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
  if (!link.value || busy.value) return;
  busy.value = true;
  message.value = '';
  failed.value = false;
  try {
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
