<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue';
import { useCore } from '../../services/core';
import { failure, type TrackView } from '../../services/contracts';
import type {
  ProfilePlayRecordPeriod,
  ProfilePlayRecordTarget,
  ProfilePlayRecordView,
} from '../../services/profilePlayRecord';
import Icon from '../Icon.vue';

const props = defineProps<{
  target: ProfilePlayRecordTarget;
  epoch: string;
  playable: boolean;
  liked: Set<string>;
  busyLikes: Set<string>;
  currentId?: string;
  playing: boolean;
}>();
const emit = defineEmits<{
  period: [value: ProfilePlayRecordPeriod];
  play: [tracks: TrackView[], index: number];
  like: [id: string];
  expired: [];
}>();
const core = useCore();
const view = shallowRef<ProfilePlayRecordView>();
const busy = ref(false);
const failed = ref(false);
const heading = ref<HTMLElement>();
const items = computed(() => (view.value?.state === 'ready' ? view.value.items : []));
const trackList = computed(() => items.value.map((item) => item.track));
const own = computed(
  () => props.target.uid === null || (view.value?.state === 'ready' && view.value.isSelf),
);
const count = new Intl.NumberFormat('zh-CN');

let revision = 0;
watch([() => props.epoch, () => props.target.uid, () => props.target.period], () => void load(), {
  immediate: true,
  flush: 'sync',
});
onMounted(() => heading.value?.focus({ preventScroll: true }));
onBeforeUnmount(() => revision++);

async function load() {
  const attempt = ++revision;
  const { epoch, target } = props;
  busy.value = true;
  failed.value = false;
  view.value = undefined;
  try {
    const reply = await core.profilePlayRecord(epoch, target.uid, target.period);
    if (attempt !== revision) return;
    if (reply.epoch !== epoch) throw Error('stale profile play record');
    view.value = reply.value;
  } catch (error) {
    if (attempt === revision) {
      const code = failure(error).code;
      if (code === 'session_expired' || code === 'profile_changed') emit('expired');
      failed.value = true;
    }
  } finally {
    if (attempt === revision) busy.value = false;
  }
}
</script>

<template>
  <section class="listening-rank" aria-label="听歌排行" :aria-busy="busy">
    <h1 ref="heading" tabindex="-1">{{ own ? '我的听歌排行' : `${target.name}的听歌排行` }}</h1>
    <div class="rank-periods" role="group" aria-label="排行时间">
      <button
        v-for="option in [
          { id: 'week', label: '最近一周' },
          { id: 'all', label: '所有时间' },
        ] as const"
        :key="option.id"
        class="text-button"
        type="button"
        :aria-pressed="target.period === option.id"
        @click="target.period !== option.id && emit('period', option.id)"
      >
        {{ option.label }}
      </button>
    </div>
    <p v-if="busy" role="status">正在读取听歌排行…</p>
    <p v-else-if="failed" role="alert">
      无法读取听歌排行
      <button class="text-button" type="button" @click="load">重试</button>
    </p>
    <p v-else-if="view?.state === 'denied'" role="status">此听歌排行不可查看</p>
    <p v-else-if="!items.length" class="muted">暂无听歌排行</p>
    <ol v-else class="rank-items">
      <li v-for="(item, index) in items" :key="`${item.track.id}:${index}`" class="rank-row">
        <span aria-hidden="true" class="muted rank-position">{{ index + 1 }}</span>
        <button
          class="rank-song text-button"
          type="button"
          :class="{ 'rank-current': item.track.id === currentId }"
          :disabled="!playable || item.track.missing"
          :aria-label="`${item.track.id === currentId && playing ? '暂停' : '播放'} ${item.track.name}`"
          @click="emit('play', trackList, index)"
        >
          <span class="rank-cover"
            ><img
              v-if="item.track.coverUrl"
              :src="item.track.coverUrl"
              alt=""
              loading="lazy"
              referrerpolicy="no-referrer" /><Icon v-else name="music"
          /></span>
          <span class="rank-meta"
            ><strong>{{ item.track.name }}</strong
            ><span class="muted">{{ item.track.artists.join(' / ') }}</span></span
          >
        </button>
        <button
          class="icon-button"
          :class="{ 'rank-current': liked.has(item.track.id) }"
          type="button"
          :aria-label="`${liked.has(item.track.id) ? '取消喜欢' : '喜欢'} ${item.track.name}`"
          :aria-pressed="liked.has(item.track.id)"
          :disabled="busyLikes.has(item.track.id) || item.track.missing"
          @click="emit('like', item.track.id)"
        >
          <Icon name="heart" />
        </button>
        <span v-if="item.playCount !== undefined" class="muted rank-count"
          ><Icon name="play" />
          <span aria-label="播放量">{{ count.format(item.playCount) }}</span></span
        >
      </li>
    </ol>
  </section>
</template>

<style scoped>
.rank-periods {
  display: flex;
  gap: 1.25rem;
  margin-block: 1rem;
}
.rank-periods [aria-pressed='true'] {
  color: var(--accent);
  border-bottom: 2px solid currentColor;
  font-weight: 600;
}
.rank-items {
  list-style: none;
  padding: 0;
  margin: 0;
}
.rank-row {
  display: flex;
  min-height: 56px;
  align-items: center;
  gap: 0.625rem;
  padding-inline: 0.625rem;
  border-radius: 6px;
}
.rank-row:nth-child(odd) {
  background: var(--surface-alt);
}
.rank-position {
  width: 2.25rem;
  text-align: center;
  font-variant-numeric: tabular-nums;
}
.rank-song {
  display: flex;
  flex: 1;
  min-width: 0;
  gap: 0.625rem;
  text-align: start;
  align-items: center;
}
.rank-cover {
  display: grid;
  flex: 0 0 36px;
  height: 36px;
  place-items: center;
  border-radius: 4px;
  overflow: hidden;
  background: var(--surface-alt);
}
.rank-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.rank-meta {
  display: grid;
  min-width: 0;
}
.rank-meta > * {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.rank-count {
  display: flex;
  align-items: center;
  justify-content: end;
  min-width: 5rem;
  gap: 0.25rem;
  font-variant-numeric: tabular-nums;
}
.rank-count .icon {
  width: 0.85em;
}
.rank-current {
  color: var(--accent);
}
</style>
