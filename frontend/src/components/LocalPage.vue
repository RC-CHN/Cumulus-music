<script setup lang="ts">
import { ref, computed, nextTick } from 'vue';
import Icon from './Icon.vue';
import LocalMatchDialog from './LocalMatchDialog.vue';
import type { LocalMatchingModel } from '../composables/useLocalMatching';
import type { LocalModel } from '../composables/useLocal';
import { useLocalSearch } from '../composables/useLocalSearch';
import type { LocalGroup, LocalQuery, LocalSort, LocalRoot } from '../services/local';
import { time } from '../services/player';
const props = defineProps<{
  model: LocalModel;
  matching: LocalMatchingModel;
  playable: boolean;
  currentId?: string;
}>();
const emit = defineEmits<{
  play: [query: LocalQuery, revision: string, id: string | null];
  comments: [id: string, title: string];
}>();
const { view, query, loading, busy, error, readError, notice } = props.model;
const search = useLocalSearch(props.model);
const { keyword } = search;
const searchInput = ref<HTMLInputElement>();
const pageRoot = ref<HTMLElement>();
const ownsVisit = props.model.captureVisit();
const readLocked = computed(() => loading.value || !!readError.value || !view.value);
const canPlay = computed(
  () => props.playable && !busy.value && !readLocked.value && !!view.value?.playable,
);
defineExpose({ checkpoint: search.flush });
async function runAction(action: () => Promise<void>) {
  const origin = document.activeElement;
  await action();
  await nextTick();
  if (
    ownsVisit() &&
    origin instanceof HTMLElement &&
    pageRoot.value?.contains(origin) &&
    document.activeElement === document.body &&
    !origin.matches(':disabled')
  ) {
    origin.focus({ preventScroll: true });
    origin.scrollIntoView({ block: 'nearest' });
  }
}
async function clearSearch() {
  search.clear();
  await nextTick();
  if (ownsVisit()) searchInput.value?.focus({ preventScroll: true });
}
async function retryRead() {
  const origin = document.activeElement;
  await props.model.refresh();
  await nextTick();
  if (
    ownsVisit() &&
    origin instanceof HTMLElement &&
    pageRoot.value?.isConnected &&
    document.activeElement === document.body
  ) {
    const target = readError.value
      ? pageRoot.value.querySelector<HTMLElement>('[data-local-read-error] button:not(:disabled)')
      : searchInput.value;
    target?.focus({ preventScroll: true });
    target?.scrollIntoView({ block: 'nearest' });
  }
}
const tabs: { kind: LocalGroup; name: string }[] = [
  { kind: 'list', name: '歌曲' },
  { kind: 'artist', name: '歌手' },
  { kind: 'album', name: '专辑' },
  { kind: 'folder', name: '文件夹' },
];
const scanning = computed(() =>
  view.value?.roots.some((r) => r.enabled && ['pending', 'scanning'].includes(r.status)),
);
function play(id: string | null = null) {
  if (
    view.value &&
    canPlay.value &&
    (id === null || view.value.tracks.some((row) => row.track.id === id && !row.track.missing))
  )
    emit('play', { ...query.value }, view.value.revision, id);
}
function sort(field: LocalSort) {
  props.model.filter({
    sort: field,
    descending: query.value.sort === field ? !query.value.descending : false,
  });
}
function status(root: LocalRoot) {
  if (!root.enabled) return '已暂停自动扫描';
  if (root.rootMissing) return '目录暂不可用，文件记录已保留';
  if (root.status === 'pending') return '等待扫描';
  if (root.status === 'scanning')
    return root.progress?.stage === 'probing'
      ? `正在处理 ${root.progress.processed} / ${root.progress.total}`
      : root.progress?.stage === 'committing'
        ? '正在保存列表'
        : '正在查找音乐';
  if (root.error) return root.error;
  return `已更新 ${root.updated}，未变化 ${root.unchanged}${root.failures ? `，${root.failures} 项无法读取` : ''}`;
}
</script>
<template>
  <section
    ref="pageRoot"
    class="local-page"
    aria-label="本地音乐"
    :data-local-loading="loading"
    :data-local-ready="!!view && !readError"
    :aria-busy="loading"
  >
    <header class="local-heading">
      <h1>本地音乐</h1>
      <span>共 {{ view?.total ?? 0 }} 首</span
      ><span role="status">{{ scanning ? '正在更新列表' : '' }}</span
      ><button class="text-button" :disabled="busy" @click="runAction(model.choose)">
        选择目录
      </button>
    </header>
    <div class="local-toolbar">
      <button class="primary" :disabled="!canPlay" @click="play()">
        <Icon name="play" />播放全部
      </button>
      <button
        class="secondary"
        :disabled="
          !matching.ready.value ||
          matching.busy.value ||
          matching.unfinished.value ||
          readLocked ||
          !view?.playable
        "
        :title="matching.ready.value ? '匹配未关联的本地音乐' : '登录后可关联线上歌曲'"
        @click="matching.start"
      >
        匹配音乐
      </button>
      <button
        class="secondary"
        :disabled="busy || !view?.roots.some((r) => r.enabled)"
        @click="runAction(() => model.command({ kind: 'rescan' }))"
      >
        重新扫描
      </button>
      <form class="local-search" @submit.prevent="search.flush">
        <input
          ref="searchInput"
          :key="query.group"
          v-model="keyword"
          aria-label="搜索本地音乐"
          placeholder="搜索本地音乐"
          @input="search.input(($event.target as HTMLInputElement).value)"
          @compositionstart="search.compositionStart"
          @compositionend="search.compositionEnd(($event.target as HTMLInputElement).value)"
        />
        <button
          v-if="keyword"
          type="button"
          class="icon-button"
          aria-label="清空本地搜索"
          @click="clearSearch"
        >
          <Icon name="close" /></button
        ><button class="icon-button" aria-label="搜索本地音乐列表">
          <Icon name="search" />
        </button>
      </form>
    </div>
    <div v-if="matching.view.value?.job" class="local-match-progress" role="status">
      <span
        >{{
          {
            queued: '等待匹配',
            running: '正在匹配',
            paused: '匹配已暂停',
            failed: '匹配未完成，请重试',
            complete: '匹配完成',
            cancelled: '匹配已取消',
          }[matching.view.value.job.phase]
        }}
        · {{ matching.view.value.job.processed }} / {{ matching.view.value.job.total }}，成功
        {{ matching.view.value.job.matched
        }}<template v-if="matching.view.value.job.unmatched"
          >，未匹配 {{ matching.view.value.job.unmatched }}</template
        ><template v-if="matching.view.value.job.skipped"
          >，已跳过 {{ matching.view.value.job.skipped }}</template
        ></span
      >
      <progress
        v-if="matching.unfinished.value"
        :value="matching.view.value.job.processed"
        :max="matching.view.value.job.total || 1"
      />
      <button
        v-if="matching.running.value"
        class="text-button"
        :disabled="matching.busy.value"
        @click="matching.control('pause')"
      >
        暂停匹配
      </button>
      <button
        v-else-if="matching.unfinished.value"
        class="text-button"
        :disabled="matching.busy.value"
        @click="matching.control('resume')"
      >
        {{ matching.view.value.job.phase === 'failed' ? '重试匹配' : '继续匹配' }}
      </button>
      <button
        v-if="matching.unfinished.value"
        class="text-button"
        :disabled="matching.busy.value"
        @click="matching.control('cancel')"
      >
        取消匹配
      </button>
    </div>
    <p v-if="matching.error.value" class="error-banner" role="alert">
      {{ matching.error.value
      }}<button class="text-button" @click="matching.refresh">重新加载</button>
    </p>
    <div class="local-tabs" aria-label="本地音乐分类">
      <button
        v-for="tab in tabs"
        :key="tab.kind"
        :aria-pressed="query.group === tab.kind"
        @click="search.group(tab.kind)"
      >
        {{ tab.name }}</button
      ><label
        ><input
          type="checkbox"
          :checked="query.includeMissing"
          @change="model.filter({ includeMissing: ($event.target as HTMLInputElement).checked })"
        />显示不可用文件</label
      >
    </div>
    <details class="local-roots">
      <summary>管理音乐目录（{{ view?.roots.length ?? 0 }}）</summary>
      <p>自动扫描每 30 秒检查一次。暂停扫描保留列表和原文件。</p>
      <div v-for="root in view?.roots" :key="root.id" class="local-root">
        <div>
          <strong :title="root.path">{{ root.path }}</strong
          ><small>{{ status(root) }}</small
          ><progress
            v-if="root.status === 'scanning' && root.progress?.total"
            :value="root.progress.processed"
            :max="root.progress.total"
          />
        </div>
        <button
          class="text-button"
          :disabled="busy"
          @click="
            runAction(() =>
              model.command({ kind: root.enabled ? 'disable' : 'enable', id: root.id }),
            )
          "
        >
          {{ root.enabled ? '暂停扫描' : '恢复扫描' }}
        </button>
      </div>
    </details>
    <p v-if="notice" class="notice" role="status">{{ notice }}</p>
    <div v-if="error" class="error-banner" role="alert" data-local-command-error>
      {{ error }}
    </div>
    <div v-if="readError && view" class="error-banner" role="alert" data-local-read-error>
      {{ readError
      }}<button class="text-button" :disabled="loading" @click="retryRead">重试读取</button>
    </div>
    <div v-if="loading && !view" class="empty-state" role="status">
      <span class="spinner" />正在读取音乐库…
    </div>
    <div v-else-if="!view && readError" class="error-state" role="alert" data-local-read-error>
      <h2>本地音乐暂时无法读取</h2>
      <p>{{ readError }}</p>
      <button class="primary" :disabled="loading" @click="retryRead">重试读取</button>
    </div>
    <div v-else-if="!view?.total && !query.keyword && !query.groupKey" class="empty-state">
      <Icon name="music" />
      <h2>让收藏的音乐回到播放列表</h2>
      <p>选择音乐目录，支持 CUE 分轨和 NCM 文件。</p>
      <button class="primary" :disabled="busy" @click="model.choose">选择音乐目录</button>
    </div>
    <div v-else class="local-browser">
      <aside v-if="query.group !== 'list'" class="local-groups" aria-label="本地分组">
        <button :aria-pressed="query.groupKey === null" @click="model.filter({ groupKey: null })">
          全部</button
        ><button
          v-for="group in view?.groups"
          :key="group.key"
          :aria-pressed="query.groupKey === group.key"
          :title="group.name"
          @click="model.filter({ groupKey: group.key })"
        >
          <span>{{ group.name }}</span
          ><small>{{ group.count }}</small>
        </button>
      </aside>
      <div class="local-tracks">
        <table>
          <thead>
            <tr>
              <th>#</th>
              <th>
                <button @click="sort('title')">
                  音乐标题 {{ query.sort === 'title' ? (query.descending ? '↓' : '↑') : '' }}
                </button>
              </th>
              <th><button @click="sort('artist')">歌手</button></th>
              <th><button @click="sort('album')">专辑</button></th>
              <th><button @click="sort('format')">格式</button></th>
              <th><button @click="sort('duration')">时长</button></th>
              <th v-if="matching.ready.value">线上关联</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="(row, index) in view?.tracks"
              :key="row.track.id"
              :data-local-id="row.track.id"
              :class="{ current: currentId === row.track.id, unavailable: row.track.missing }"
              @dblclick="!row.track.missing && play(row.track.id)"
            >
              <td>
                <button
                  class="icon-button"
                  :disabled="!canPlay || row.track.missing"
                  :aria-label="`播放本地歌曲 ${row.track.name}`"
                  @click="play(row.track.id)"
                >
                  <Icon v-if="currentId === row.track.id" name="volume" /><span v-else>{{
                    query.offset + index + 1
                  }}</span>
                </button>
              </td>
              <td>
                <button
                  class="local-title"
                  :disabled="!canPlay || row.track.missing"
                  :title="`${row.folder}/${row.fileName}`"
                  @click="play(row.track.id)"
                >
                  {{ row.track.name }}</button
                ><small v-if="row.cue" class="local-badge">CUE</small
                ><small v-if="row.encrypted" class="local-badge">NCM</small
                ><small v-if="row.track.missing">{{
                  row.unreadable ? '无法读取' : '文件缺失'
                }}</small>
              </td>
              <td :title="row.track.artists.join(' / ')">
                {{ row.track.artists.join(' / ') || '未知歌手' }}
              </td>
              <td :title="row.track.album">{{ row.track.album || '未知专辑' }}</td>
              <td>{{ row.format.toUpperCase() }}</td>
              <td>{{ time(row.track.durationMs / 1000) }}</td>
              <td v-if="matching.ready.value" class="local-online" @dblclick.stop>
                <button
                  class="text-button"
                  :disabled="
                    readLocked || row.track.missing || matching.busy.value || matching.running.value
                  "
                  :aria-label="`关联本地歌曲 ${row.track.name}`"
                  @click="matching.open(row)"
                >
                  {{ matching.link(row) ? '已关联' : '关联' }}</button
                ><button
                  v-if="matching.link(row)"
                  class="icon-button"
                  :disabled="readLocked"
                  :aria-label="`${row.track.name} 的线上评论`"
                  @click="
                    emit(
                      'comments',
                      matching.link(row)!.songId,
                      matching.link(row)?.track?.name || row.track.name,
                    )
                  "
                >
                  <Icon name="comment" />
                </button>
              </td>
            </tr>
          </tbody>
        </table>
        <p v-if="!view?.tracks.length" class="empty-state">没有匹配的本地音乐</p>
      </div>
    </div>
    <footer v-if="view?.total" class="local-pagination">
      <button
        class="text-button"
        :disabled="!query.offset || readLocked"
        @click="model.page(Math.max(0, query.offset - query.limit))"
      >
        上一页</button
      ><span
        >{{ Math.min(query.offset + 1, view.total) }}–{{
          Math.min(query.offset + query.limit, view.total)
        }}
        / {{ view.total }}</span
      ><button
        class="text-button"
        :disabled="!view.more || readLocked"
        @click="model.page(query.offset + query.limit)"
      >
        下一页
      </button>
    </footer>
  </section>
  <LocalMatchDialog v-if="matching.target.value" :model="matching" />
</template>
