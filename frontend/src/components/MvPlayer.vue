<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import type { MvModel } from '../composables/useMv';
import VideoCanvas from './VideoCanvas.vue';
import MvRecommendations from './MvRecommendations.vue';
import Icon from './Icon.vue';
import type { TrackView } from '../services/contracts';
import { videoTime } from '../services/mv';
import { bytesLabel } from '../services/downloads';
const props = withDefaults(
  defineProps<{
    model: MvModel;
    mini: boolean;
    playable?: boolean;
    songLikes?: ReadonlySet<string>;
    songBusy?: ReadonlySet<string>;
    songNotice?: string;
  }>(),
  {
    playable: false,
    songLikes: () => new Set<string>(),
    songBusy: () => new Set<string>(),
    songNotice: '',
  },
);
const emit = defineEmits<{
  expand: [];
  downloads: [];
  comments: [];
  related: [mvId: string, songId: string];
  relatedLike: [song: TrackView];
  open: [id: string];
}>();
const { video, detail, detailLoading, error, videoError, videoReadError, notice, busy } =
  props.model;
const fullscreen = ref(false),
  downloadOpen = ref(false);
const displayTitle = computed(() => detail.value?.card.title ?? props.model.localCard.value?.name);
const related = computed(() => detail.value?.relatedSongs[0]);
function playRelated() {
  if (!related.value || !detail.value) return;
  fullscreen.value = false;
  emit('related', detail.value.card.id, related.value.id);
}
const loaded = computed(
  () => !!video.value && ['playing', 'paused', 'ended'].includes(video.value.state),
);
const waiting = computed(
  () => !!video.value && ['resolving', 'loading', 'file_operation'].includes(video.value.state),
);
function toggle() {
  void props.model.action(
    video.value?.paused || video.value?.state === 'ended'
      ? { kind: 'play' }
      : { kind: 'pause', paused: true },
  );
}
function key(event: KeyboardEvent) {
  if (event.key === 'Escape' && fullscreen.value) {
    fullscreen.value = false;
    event.stopImmediatePropagation();
  }
}
watch(
  () => props.mini,
  () => {
    fullscreen.value = false;
    downloadOpen.value = false;
  },
);
onMounted(() => window.addEventListener('keydown', key, true));
onUnmounted(() => window.removeEventListener('keydown', key, true));
</script>
<template>
  <section
    class="mv-detail"
    :class="{ 'mv-mini': mini, 'mv-fullscreen': fullscreen }"
    aria-label="MV播放器"
  >
    <div class="mv-screen">
      <VideoCanvas :model="model" />
      <div class="mv-top">
        <button v-if="mini" class="video-button" @click="emit('expand')">返回 MV</button
        ><span v-else class="ellipsis">{{ displayTitle ?? 'MV' }}</span
        ><button class="video-button" aria-label="关闭视频" @click="model.action({ kind: 'stop' })">
          ×
        </button>
      </div>
      <div v-if="waiting || video?.buffering" class="video-message" role="status">
        <template v-if="video?.state === 'file_operation'">
          正在处理本地视频文件，完成后恢复播放
          <button class="video-button" @click="emit('downloads')">查看文件操作</button>
        </template>
        <template v-else><span class="spinner" />正在加载视频…</template>
      </div>
      <div
        v-else-if="video?.state === 'error' || videoError || videoReadError"
        class="video-message"
        role="alert"
      >
        {{ videoError || videoReadError || video?.error
        }}<button class="video-button" @click="model.action({ kind: 'play' })">重试</button>
      </div>
      <button
        v-else-if="video?.state === 'idle'"
        class="video-message video-button"
        @click="model.action({ kind: 'play' })"
      >
        ▶ 播放 MV
      </button>
      <div v-if="!mini && related" class="mv-related-song">
        <button
          class="video-button ellipsis"
          :disabled="!playable"
          aria-label="播放MV关联歌曲"
          @click="playRelated"
        >
          ♫ 相关歌曲：{{ related.name }} · {{ related.artists.join(' / ') }}
        </button>
        <button
          class="video-button"
          :aria-label="songLikes.has(related.id) ? '取消喜欢MV关联歌曲' : '喜欢MV关联歌曲'"
          :aria-pressed="songLikes.has(related.id)"
          :disabled="songBusy.has(related.id)"
          @click="emit('relatedLike', related)"
        >
          <Icon name="heart" />
        </button>
      </div>
      <div class="video-bottom">
        <input
          class="video-progress"
          type="range"
          min="0"
          :max="video?.duration ?? 0"
          step="0.1"
          :value="video?.position ?? 0"
          :disabled="!loaded"
          aria-label="MV播放进度"
          @change="
            model.action({
              kind: 'seek',
              seconds: Number(($event.target as HTMLInputElement).value),
            })
          "
        />
        <div class="video-controls">
          <button
            class="video-button"
            :aria-label="video?.paused ? '播放MV' : '暂停MV'"
            :disabled="!loaded && !waiting"
            @click="toggle"
          >
            {{ video?.paused ? '▶' : 'Ⅱ' }}</button
          ><span class="video-time"
            >{{ videoTime(video?.position ?? null) }} /
            {{ videoTime(video?.duration ?? null) }}</span
          ><span class="video-flex" />
          <template v-if="!mini"
            ><label class="video-volume"
              >音量<input
                type="range"
                min="0"
                max="100"
                :value="video?.volume ?? 60"
                aria-label="MV音量"
                @change="
                  model.action({
                    kind: 'volume',
                    value: Number(($event.target as HTMLInputElement).value),
                  })
                " /></label
            ><select
              aria-label="MV播放速度"
              :value="video?.speed ?? 1"
              @change="
                model.action({
                  kind: 'speed',
                  value: Number(($event.target as HTMLSelectElement).value),
                })
              "
            >
              <option v-for="speed in [0.5, 0.75, 1, 1.25, 1.5, 2]" :key="speed" :value="speed">
                {{ speed }}×
              </option></select
            ><select
              v-if="detail?.playResolutions.length"
              aria-label="MV清晰度"
              :value="video?.requestedResolution"
              :disabled="waiting"
              @change="
                model.action({
                  kind: 'quality',
                  resolution: Number(($event.target as HTMLSelectElement).value),
                })
              "
            >
              <option v-for="br in detail.playResolutions" :key="br" :value="br">{{ br }}P</option>
            </select></template
          >
          <button
            class="video-button"
            :aria-label="fullscreen ? '退出MV全屏' : 'MV全屏'"
            @click="fullscreen = !fullscreen"
          >
            {{ fullscreen ? '收起' : '全屏' }}
          </button>
        </div>
      </div>
    </div>
    <div v-if="!mini" class="mv-info">
      <h1>{{ displayTitle ?? (detailLoading ? '正在读取 MV…' : 'MV') }}</h1>
      <p class="muted">
        {{ (detail?.card.artists ?? model.localCard.value?.artists)?.join(' / ')
        }}<span v-if="detail?.publishedDate"> · {{ detail.publishedDate }}</span>
      </p>
      <p class="mv-play-facts">
        <span v-if="video?.local">本地文件 · </span
        ><span v-if="video?.actualResolution"
          >实际 {{ video.actualResolution }}P<span
            v-if="video.actualResolution !== video.requestedResolution"
            >（请求 {{ video.requestedResolution }}P）</span
          ></span
        ><span v-if="detail?.cached"> · 缓存信息，网络暂不可用</span>
      </p>
      <div class="mv-info-actions">
        <button
          class="download-button mv-like"
          :class="{ liked: model.social.liked.value }"
          :aria-label="model.social.liked.value ? '取消MV点赞' : '点赞MV'"
          :aria-pressed="model.social.liked.value ?? false"
          :disabled="
            model.social.busy.value ||
            model.social.liked.value === null ||
            !!model.social.pending.value
          "
          @click="model.social.like(!model.social.liked.value)"
        >
          <Icon name="thumb" />{{ model.social.liked.value ? '已赞' : '赞' }}
          {{ model.social.count.value || '' }}
        </button>
        <button
          class="download-button"
          aria-label="MV 评论"
          :disabled="!detail"
          @click="emit('comments')"
        >
          MV 评论<span v-if="detail?.commentCount"> {{ detail.commentCount }}</span>
        </button>
        <button
          class="download-button"
          :disabled="!detail || busy"
          @click="detail && model.collect(detail.card.id, !detail.card.subscribed)"
        >
          {{ detail?.card.subscribed ? '已收藏 · 取消' : '收藏 MV' }}</button
        ><button class="download-button" :disabled="!detail" @click="downloadOpen = !downloadOpen">
          下载 MV</button
        ><button class="text-button" @click="emit('downloads')">下载管理</button
        ><button class="text-button" :disabled="waiting" @click="model.action({ kind: 'refresh' })">
          刷新音源
        </button>
      </div>
      <p v-if="model.social.error.value" class="form-warning" role="alert">
        {{ model.social.error.value }}
        <button
          class="text-button"
          :disabled="model.social.busy.value"
          @click="model.social.refresh"
        >
          刷新点赞状态
        </button>
      </p>
      <p v-if="model.social.pending.value" class="notice" role="status">
        点赞结果尚未确认
        <button
          class="text-button"
          :disabled="model.social.busy.value"
          @click="model.social.like(model.social.pending.value.desired, true)"
        >
          核对 MV 点赞
        </button>
      </p>
      <p v-if="detail?.relatedPrivilegeFailed" class="form-warning">
        相关歌曲权限暂未刷新，播放时会重新检查可用音源。
      </p>
      <p v-if="songNotice" class="form-warning" role="status">{{ songNotice }}</p>
      <div v-if="downloadOpen && detail" class="mv-download-options">
        <h2>选择下载清晰度</h2>
        <p v-if="!detail.downloads.length" class="muted">当前账号没有可用的下载清晰度</p>
        <button
          v-for="choice in detail.downloads"
          :key="choice.resolution"
          class="download-button"
          :disabled="busy || !choice.available"
          :title="choice.reason ?? ''"
          @click="model.download(choice.resolution)"
        >
          {{ choice.resolution }}P · {{ bytesLabel(choice.estimatedBytes)
          }}<span v-if="choice.reason"> · {{ choice.reason }}</span>
        </button>
      </div>
      <p v-if="error" class="form-warning" role="alert">
        {{ error
        }}<button
          v-if="!detail"
          class="text-button"
          :disabled="detailLoading"
          @click="model.readDetail"
        >
          重新读取MV资料
        </button>
      </p>
      <p v-if="notice" class="notice" role="status">{{ notice }}</p>
      <div v-for="op in model.page.value?.operations" :key="op.id">
        <p v-if="op.id === detail?.card.id" class="notice">
          收藏操作结果待确认
          <button
            class="text-button"
            :disabled="busy"
            @click="model.collect(op.id, op.desired, true)"
          >
            核对状态
          </button>
        </p>
      </div>
      <p v-if="detail?.description" class="mv-description">{{ detail.description }}</p>
      <MvRecommendations :model="model.social" @open="emit('open', $event)" />
    </div>
  </section>
</template>
