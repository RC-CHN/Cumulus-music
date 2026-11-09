<script setup lang="ts">
import { onMounted, onScopeDispose, ref } from 'vue';
import { useControlFocus } from '../composables/useControlFocus';
import { useLocalBatch } from '../composables/useLocalBatch';
import { usePageEntryFocus } from '../composables/usePageEntryFocus';
import { LOCAL_BATCH_PAGE_SIZE, type LocalQuery } from '../services/local';
import { time } from '../services/player';

const props = defineProps<{ query: LocalQuery; current: (query: LocalQuery) => boolean }>();
const source = props.query;
const live = () => props.current(source);
const model = useLocalBatch(source, undefined, live);
const {
  page,
  selected,
  loading,
  busy,
  locked,
  readError,
  commandError,
  notice,
  selectionCount,
  pageSelection,
} = model;
const root = ref<HTMLElement>();
const heading = ref<HTMLHeadingElement>();
const list = ref<HTMLElement>();
const previous = ref<HTMLButtonElement>();
const next = ref<HTMLButtonElement>();
usePageEntryFocus(root, () => !!page.value || !!readError.value, live);
const focus = useControlFocus(live, () => heading.value);
const pageFocus = useControlFocus(
  live,
  () => [previous.value, next.value].find((button) => button && !button.disabled) ?? heading.value,
);
onMounted(() => void model.load());
onScopeDispose(model.dispose);
function toggle(event: Event, id?: string) {
  model.toggle(id);
  const input = event.currentTarget as HTMLInputElement;
  input.checked = id === undefined ? pageSelection.value.all : selected.value.has(id);
  if (id === undefined) input.indeterminate = pageSelection.value.some;
}
async function move(event: Event, direction: -1 | 1) {
  const offset = page.value?.offset;
  await pageFocus.run(event, async () => {
    await model.move(direction);
    if (live() && list.value && offset !== page.value?.offset) list.value.scrollTop = 0;
  });
}
</script>

<template>
  <section
    ref="root"
    class="local-batch-page"
    data-local-batch
    aria-labelledby="local-batch-heading"
    :aria-busy="loading || busy"
  >
    <header class="local-heading">
      <h1 id="local-batch-heading" ref="heading" tabindex="-1">批量操作</h1>
      <span>本地音乐{{ page ? ` · 共 ${page.total} 首` : '' }}</span>
    </header>
    <p v-if="model.query.keyword.trim()" class="local-batch-filter">
      搜索：{{ model.query.keyword }}
    </p>
    <div class="local-batch-actions" aria-label="批量操作选项">
      <label>
        <input
          type="checkbox"
          :checked="pageSelection.all"
          :indeterminate.prop="pageSelection.some"
          :disabled="locked || !page?.tracks.length"
          @change="toggle($event)"
        />
        选择本页全部
      </label>
      <span role="status" data-local-batch-selection>已选择 {{ selectionCount }} 首</span>
      <button
        type="button"
        class="secondary"
        data-local-batch-remove
        :disabled="locked || !selectionCount"
        @click="focus.run($event, model.remove)"
      >
        从列表中删除
      </button>
    </div>
    <p class="local-batch-note">只从本地音乐列表移除，保留源文件。</p>
    <p v-if="busy" class="local-batch-note" role="status">正在从列表中删除…</p>
    <p v-if="notice" class="notice" role="status" data-local-batch-notice>{{ notice }}</p>
    <p v-if="commandError" class="error-banner" role="alert" data-local-batch-command-error>
      {{ commandError }}
    </p>
    <div v-if="readError" class="error-banner" role="alert" data-local-batch-read-error>
      {{ readError }}
      <button class="text-button" type="button" @click="focus.run($event, model.retry)">
        重试读取
      </button>
    </div>
    <p v-if="loading && !page" class="local-batch-note" role="status">正在读取本地音乐…</p>
    <p v-else-if="page && !page.total" class="local-batch-note">没有符合条件的本地音乐</p>
    <div v-if="page?.tracks.length" ref="list" class="local-batch-list local-tracks">
      <table aria-label="本页本地歌曲">
        <thead>
          <tr>
            <th scope="col" aria-label="选择" />
            <th scope="col">音乐标题</th>
            <th scope="col">歌手</th>
            <th scope="col">专辑</th>
            <th scope="col">时长</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="row in page.tracks"
            :key="row.track.id"
            :data-local-batch-id="row.track.id"
            :class="{ unavailable: row.track.missing }"
          >
            <td>
              <input
                type="checkbox"
                :aria-label="`选择 ${row.track.name}`"
                :checked="selected.has(row.track.id)"
                :disabled="locked"
                @change="toggle($event, row.track.id)"
              />
            </td>
            <td :title="row.track.name">{{ row.track.name }}</td>
            <td>{{ row.track.artists.join(' / ') || '未知歌手' }}</td>
            <td>{{ row.track.album || '未知专辑' }}</td>
            <td>{{ time(row.track.durationMs / 1000) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
    <nav
      v-if="page && (page.offset > 0 || page.more)"
      class="local-batch-pager"
      aria-label="本地音乐批量分页"
    >
      <button
        type="button"
        class="secondary"
        ref="previous"
        data-local-batch-previous
        :disabled="locked || page.offset === 0"
        @click="move($event, -1)"
      >
        上一页
      </button>
      <span>第 {{ Math.floor(page.offset / LOCAL_BATCH_PAGE_SIZE) + 1 }} 页</span>
      <button
        type="button"
        class="secondary"
        ref="next"
        data-local-batch-next
        :disabled="locked || !page.more"
        @click="move($event, 1)"
      >
        下一页
      </button>
    </nav>
  </section>
</template>

<style scoped>
.local-batch-page {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  min-width: 0;
  overflow: auto;
  padding: 8px 0 18px;
}
.local-batch-actions,
.local-batch-pager {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  margin: 12px 0 4px;
}
.local-batch-actions label {
  align-items: center;
  display: inline-flex;
  gap: 6px;
}
.local-batch-filter,
.local-batch-note {
  color: var(--muted);
  font-size: 13px;
  margin: 6px 0;
  overflow-wrap: anywhere;
}
.local-batch-list {
  flex: 1 1 auto;
  min-height: 80px;
  overflow: auto;
  overscroll-behavior: contain;
}
.local-batch-list :is(td, th):first-child {
  text-align: center;
  width: 36px;
}
.local-batch-list :is(td, th):last-child {
  width: 50px;
}
.local-batch-list th {
  position: sticky;
  top: 0;
  background: var(--surface);
}
</style>
