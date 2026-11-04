<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import type { Quality } from '../../services/player';
import { useCore } from '../../services/core';
import { useArtistOnboarding } from '../../composables/useArtistOnboarding';
import { artworkAt } from '../../services/artwork';
import { modalEntered, modalLeft } from '../../services/modal';
import Icon from '../Icon.vue';

const props = defineProps<{
  account: AccountView;
  enabled: boolean;
  playerReady: boolean;
  queueRevision?: string;
  quality: Quality;
}>();
const emit = defineEmits<{ transport: [] }>();
const model = useArtistOnboarding(
  useCore(),
  () => props.account,
  () => props.enabled,
);
const { offer, candidates, selected, songs, step, saved, busy, error } = model;
const dialog = ref<HTMLDialogElement>();
const list = ref<HTMLElement>();
const selectedIds = computed(() => new Set(selected.value.map((artist) => artist.id)));
watch(
  dialog,
  (node, previous) => {
    if (previous) {
      modalLeft(previous);
      if (previous.open) previous.close();
    }
    if (node?.isConnected && !node.open) {
      node.showModal();
      modalEntered(node);
      node.querySelector<HTMLButtonElement>('[data-favor-skip]')?.focus();
    }
  },
  { flush: 'post' },
);
onBeforeUnmount(() => {
  if (dialog.value) modalLeft(dialog.value);
});
watch(step, () => list.value?.scrollTo(0, 0), { flush: 'post' });
async function result(action: 'play_now' | 'later') {
  if (!props.playerReady || !props.queueRevision) return;
  await model.result(action, props.queueRevision, props.quality);
  emit('transport');
}
</script>

<template>
  <Teleport to="body">
    <dialog
      v-if="offer"
      ref="dialog"
      class="artist-favor-dialog"
      aria-label="艺人兴趣选择"
      :data-favor-scene="offer.scene"
      :data-favor-step="step"
      @cancel.prevent="model.close"
    >
      <section class="artist-favor-panel">
        <header>
          <div>
            <p v-if="offer.resumed" class="artist-favor-subtitle">继续上次的兴趣选择</p>
            <h2 v-if="step === 'select'">欢迎来到云音乐，选择你喜欢的艺人</h2>
            <h2 v-else-if="step === 'result'">为你推荐歌曲</h2>
            <h2 v-else>正在为你准备艺人推荐</h2>
          </div>
          <button
            data-favor-skip
            class="artist-favor-secondary"
            :disabled="busy"
            @click="model.close"
          >
            {{ step === 'select' ? '跳过' : '返回首页' }}
          </button>
        </header>
        <p
          v-if="offer.saveBlocked || saved === 'uncertain'"
          class="artist-favor-warning"
          role="status"
        >
          上次保存结果无法确认，本次不会再次发送保存请求。
        </p>
        <div ref="list" class="artist-favor-scroll" :aria-busy="busy">
          <template v-if="step === 'select'">
            <p v-if="busy" class="artist-favor-status" role="status">正在读取推荐艺人…</p>
            <div v-else-if="error && !candidates.length" class="artist-favor-status">
              <p>{{ error }}</p>
              <button class="artist-favor-secondary" @click="model.read">重试读取</button>
            </div>
            <ul v-else class="artist-favor-grid">
              <li v-for="artist in candidates" :key="artist.id">
                <button
                  :data-favor-artist="artist.id"
                  :aria-pressed="selectedIds.has(artist.id)"
                  :disabled="
                    offer.saveBlocked || (!selectedIds.has(artist.id) && selected.length >= 50)
                  "
                  @click="model.toggle(artist)"
                >
                  <span class="artist-favor-art">
                    <img
                      v-if="artist.coverUrl"
                      :src="artworkAt(artist.coverUrl, 112)"
                      alt=""
                      loading="lazy"
                      referrerpolicy="no-referrer"
                    />
                    <Icon v-else name="user" />
                    <span
                      v-if="selectedIds.has(artist.id)"
                      class="artist-favor-check"
                      aria-hidden="true"
                      >✓</span
                    >
                  </span>
                  <span class="artist-favor-name" :title="artist.name">{{ artist.name }}</span>
                </button>
              </li>
            </ul>
          </template>
          <ol v-else-if="step === 'result'" class="artist-favor-songs">
            <li v-for="(song, index) in songs" :key="song.id" :data-favor-song="song.id">
              <span class="artist-favor-index">{{ index + 1 }}</span>
              <span
                ><strong>{{ song.name }}</strong
                ><small>{{ song.artists.join(' / ') }}</small></span
              >
            </li>
          </ol>
          <div v-else class="artist-favor-status">
            <p role="status">{{ busy ? '正在处理，请稍候…' : error }}</p>
            <button
              v-if="!busy && step === 'preview'"
              class="artist-favor-secondary"
              @click="model.preview"
            >
              重试读取歌曲
            </button>
          </div>
        </div>
        <p
          v-if="error && (step === 'result' || (step === 'select' && candidates.length))"
          role="status"
          class="artist-favor-warning"
        >
          {{ error }}
        </p>
        <footer v-if="step === 'select'">
          <span>已选 {{ selected.length }} 位艺人</span>
          <button
            class="primary"
            data-favor-save
            :disabled="busy || offer.saveBlocked || selected.length < 2"
            @click="model.save"
          >
            {{ selected.length < 2 ? '请选择至少2位' : '完成' }}
          </button>
        </footer>
        <footer v-else-if="step === 'result'">
          <span>共 {{ songs.length }} 首推荐歌曲</span>
          <button
            class="artist-favor-secondary"
            :disabled="busy || !playerReady || !queueRevision"
            @click="result('later')"
          >
            稍后再听
          </button>
          <button
            class="primary"
            :disabled="busy || !playerReady || !queueRevision"
            @click="result('play_now')"
          >
            立即收听
          </button>
        </footer>
      </section>
    </dialog>
  </Teleport>
</template>

<style src="./artist-onboarding.css"></style>
