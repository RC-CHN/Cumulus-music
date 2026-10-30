<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref } from 'vue';
import type { TrackView } from '../services/contracts';
import { songLink } from '../services/trackMenu';
import { useCore } from '../services/core';
import Icon from './Icon.vue';
const props = defineProps<{ track: TrackView }>();
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
  const link = songLink(props.track);
  if (!link || busy.value) return;
  busy.value = true;
  message.value = '';
  failed.value = false;
  try {
    await client.copyText(link);
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
      <h2 id="track-share-title">分享歌曲</h2>
      <button class="icon-button" aria-label="关闭分享" @click="emit('close')">
        <Icon name="close" />
      </button>
    </header>
    <div class="track-share-body">
      <strong>{{ track.name }}</strong>
      <p class="muted">{{ track.artists.join(' / ') }}</p>
      <p>复制歌曲链接，与朋友分享</p>
      <input
        aria-label="歌曲分享链接"
        :value="songLink(track)"
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
