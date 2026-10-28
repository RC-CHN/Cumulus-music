<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue';
import Icon from './Icon.vue';
import ModalTransition from './ModalTransition.vue';
import HistoryConfirm from './history/HistoryConfirm.vue';
import { useVirtualRows } from '../composables/useVirtualRows';
import type { HistoryModel } from '../composables/useHistory';
import {
  historyLabel,
  historyOutcome,
  historyPending,
  historyTabs,
  historyTime,
  type HistoryAction,
  type HistoryCommand,
  type HistoryEntry,
  type HistoryKind,
  type HistoryResource,
} from '../services/history';
import { normalizeKeyword } from '../services/keyword';
const props = defineProps<{
  model: HistoryModel;
  playable: boolean;
  currentId?: string;
  currentVoiceId?: string;
}>();
const emit = defineEmits<{
  scope: [kind: HistoryKind];
  play: [kind: HistoryKind, revision: string, ids: string[], index: number];
  open: [resource: HistoryResource];
}>();
const { page, kind, loading, busy, error, uncertain, pending, canMutate } = props.model;
const keyword = ref('');
const selected = shallowRef(new Set<string>());
const entries = computed(() => {
  const needle = normalizeKeyword(keyword.value);
  return (page.value?.entries ?? []).filter((entry) => {
    const label = historyLabel(entry);
    return !needle || normalizeKeyword(`${label.name} ${label.subtitle}`).includes(needle);
  });
});
const { container, start, end, onScroll, reset } = useVirtualRows(() => entries.value.length, 68);
const shown = computed(() => entries.value.slice(start.value, end.value));
const allSelected = computed(
  () => entries.value.length > 0 && entries.value.every((e) => selected.value.has(e.id)),
);
const listKind = computed(() => kind.value === 'song' || kind.value === 'voice');
const label = computed(() => historyTabs.find((t) => t.kind === kind.value)?.label ?? '记录');
const activeOperations = computed(() => page.value?.operations.filter(historyPending) ?? []);
const latest = computed(() => page.value?.operations.find((op) => !historyPending(op)));
const confirm = shallowRef<{ title: string; description: string; command: HistoryCommand }>();
watch(kind, () => {
  keyword.value = '';
  selected.value = new Set();
  reset();
});
watch(keyword, () => {
  selected.value = new Set();
  reset();
});
watch(
  () => page.value?.revision,
  () => {
    selected.value = new Set();
  },
);
function choose(id: string) {
  const next = new Set(selected.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  selected.value = next;
}
function toggleAll() {
  selected.value = new Set(allSelected.value ? [] : entries.value.map((e) => e.id));
}
function playing(entry: HistoryEntry) {
  return entry.resource.kind === 'song'
    ? entry.resource.track.id === props.currentId
    : entry.resource.kind === 'voice' && entry.resource.voice.id === props.currentVoiceId;
}
function play(entry?: HistoryEntry) {
  if (!page.value || !props.playable || loading.value || busy.value) return;
  const playable = entries.value.filter((e) => historyLabel(e).available);
  const chosen = listKind.value ? playable : entry ? [entry] : [];
  const index = entry ? chosen.findIndex((e) => e.id === entry.id) : 0;
  if (index < 0 || !chosen[index] || !historyLabel(chosen[index]).available) return;
  emit(
    'play',
    kind.value,
    page.value.revision,
    chosen.map((e) => e.id),
    index,
  );
}
function remove(action: HistoryAction) {
  if (!page.value || !canMutate.value) return;
  const count = action.kind === 'clear' ? page.value.entries.length : action.ids.length;
  if (!count) return;
  confirm.value = {
    title: action.kind === 'clear' ? `清空${label.value}播放记录？` : `删除 ${count} 条播放记录？`,
    description: `${action.kind === 'clear' ? '将清空这一类型的全部历史，不仅是筛选结果。' : '将移除选中的历史记录。'}${page.value.canSync && page.value.syncEnabled ? '同时请求删除当前账号的多端记录。' : '仅删除本机记录。'}音乐文件、收藏和歌单内容会保留。`,
    command: {
      kind: 'mutate',
      id: crypto.randomUUID(),
      expected_revision: page.value.revision,
      action,
    },
  };
}
function accept(id: string) {
  if (!page.value) return;
  confirm.value = {
    title: '保留当前读取的记录？',
    description:
      '这不会重发删除请求，也不代表原请求已经成功。原请求可能仍在服务端处理中；保留当前状态后，可继续管理记录。',
    command: { kind: 'accept_current', id, expected_revision: page.value.revision },
  };
}
async function submit() {
  if (!confirm.value) return;
  const input = confirm.value.command;
  const ok = await props.model.command(input);
  // A lost reply is represented in the page journal, not a tempting second
  // submit button. Native intent IDs are never regenerated on retry.
  if (ok || pending.value) confirm.value = undefined;
}
function keyboard(event: KeyboardEvent) {
  if (
    event.repeat ||
    !['Delete', 'Backspace'].includes(event.key) ||
    !(event.target instanceof HTMLElement) ||
    event.target.closest('input, textarea, select, [contenteditable], dialog')
  )
    return;
  if (selected.value.size && canMutate.value) {
    event.preventDefault();
    remove({ kind: 'delete', ids: [...selected.value] });
  }
}
</script>
<template>
  <section class="history-page" aria-label="最近播放" :aria-busy="loading" @keydown="keyboard">
    <header class="history-heading">
      <div>
        <h1>最近播放</h1>
        <p class="muted">找回听过的音乐和声音</p>
      </div>
      <button
        class="icon-button"
        aria-label="刷新播放记录"
        :disabled="loading || busy"
        @click="model.refresh"
      >
        <Icon name="refresh" />
      </button>
    </header>
    <nav class="catalog-tabs" aria-label="播放记录类型">
      <button
        v-for="tab in historyTabs"
        :key="tab.kind"
        :aria-current="kind === tab.kind ? 'page' : undefined"
        :disabled="busy"
        @click="emit('scope', tab.kind)"
      >
        {{ tab.label }}
      </button>
    </nav>
    <div class="history-tools">
      <button
        v-if="listKind"
        class="primary"
        :disabled="!playable || loading || busy || !entries.some((e) => historyLabel(e).available)"
        @click="play()"
      >
        <Icon name="play" />播放全部
      </button>
      <label class="history-search"
        ><Icon name="search" /><input
          v-model="keyword"
          aria-label="筛选播放记录"
          placeholder="搜索名称或创作者" /><button
          v-if="keyword"
          class="icon-button"
          aria-label="清空历史筛选"
          @click="keyword = ''"
        >
          <Icon name="close" /></button
      ></label>
      <button
        class="secondary"
        :disabled="!canMutate || !selected.size"
        @click="remove({ kind: 'delete', ids: [...selected] })"
      >
        删除选中<span v-if="selected.size">（{{ selected.size }}）</span>
      </button>
      <button
        class="text-button"
        :disabled="!canMutate || !page?.entries.length"
        @click="remove({ kind: 'clear' })"
      >
        清空记录
      </button>
    </div>
    <div class="history-status">
      <span aria-live="polite"
        >{{ entries.length }} 条<template v-if="keyword">匹配记录</template
        ><template v-if="loading"> · 正在读取…</template></span
      ><label :title="page?.canSync ? '合并本机和当前账号的多端记录' : '登录后可查看多端记录'"
        ><input
          type="checkbox"
          :checked="page?.syncEnabled && page?.canSync"
          :disabled="!page?.canSync || loading || busy || pending"
          @change="model.setSync(($event.target as HTMLInputElement).checked)"
        />多端记录</label
      >
    </div>
    <div v-if="error || uncertain || activeOperations.length || latest" class="history-notices">
      <div v-if="error" class="catalog-error" role="alert">
        {{ error }}<span v-if="page"> · 保留上次读取的内容</span
        ><button class="text-button" :disabled="loading || busy" @click="model.refresh">
          重新读取
        </button>
      </div>
      <div v-if="uncertain" class="history-recovery" role="status">
        <p>
          未收到操作结果。先读取本机操作记录，或取消尚未开始的请求；已经开始的请求会保留真实状态。
        </p>
        <button class="secondary" :disabled="busy || loading" @click="model.localRefresh">
          读取操作记录</button
        ><button class="secondary" :disabled="busy || loading" @click="model.cancelUnknown">
          取消未开始的请求
        </button>
      </div>
      <div v-for="op in activeOperations" :key="op.id" class="history-recovery" role="status">
        <p>{{ historyOutcome(op) }}；本机已移除 {{ op.localRemoved }} 条。</p>
        <button
          class="secondary"
          :disabled="busy || loading || !page?.syncEnabled"
          @click="model.command({ kind: 'reconcile', id: op.id })"
        >
          核对多端记录</button
        ><button
          v-if="op.lastReadAt"
          class="secondary"
          :disabled="busy || loading"
          @click="accept(op.id)"
        >
          保留当前状态
        </button>
      </div>
      <p v-if="latest" class="history-outcome" role="status">{{ historyOutcome(latest) }}</p>
    </div>
    <Transition name="catalog-tab" mode="out-in">
      <div :key="kind" class="history-content">
        <div
          v-if="entries.length"
          class="history-table"
          role="table"
          :aria-label="`${label}播放记录`"
          :aria-rowcount="entries.length + 1"
          aria-colcount="5"
        >
          <div class="history-row history-table-heading" role="row">
            <div role="columnheader">
              <input
                type="checkbox"
                aria-label="选择筛选后的全部记录"
                :checked="allSelected"
                :indeterminate="selected.size > 0 && !allSelected"
                @change="toggleAll"
              />
            </div>
            <span role="columnheader">{{ label }} / 创作者</span
            ><span role="columnheader">播放时间</span><span role="columnheader">设备</span
            ><span role="columnheader">操作</span>
          </div>
          <div ref="container" class="history-scroll" role="rowgroup" @scroll.passive="onScroll">
            <div class="history-spacer" :style="{ height: `${entries.length * 68}px` }">
              <div class="history-window" :style="{ transform: `translateY(${start * 68}px)` }">
                <div
                  v-for="(entry, offset) in shown"
                  :key="entry.id"
                  class="history-row"
                  :class="{ 'is-playing': playing(entry), 'is-selected': selected.has(entry.id) }"
                  role="row"
                  :aria-rowindex="start + offset + 2"
                  @dblclick="play(entry)"
                >
                  <div role="cell" @dblclick.stop>
                    <input
                      type="checkbox"
                      :aria-label="`选择 ${historyLabel(entry).name}`"
                      :checked="selected.has(entry.id)"
                      @change="choose(entry.id)"
                    />
                  </div>
                  <div class="history-name-cell" role="cell">
                    <img
                      v-if="historyLabel(entry).cover"
                      :src="historyLabel(entry).cover!"
                      alt=""
                      loading="lazy"
                      decoding="async"
                    /><span v-else class="history-cover"><Icon name="music" /></span>
                    <div>
                      <button
                        class="history-name"
                        :title="historyLabel(entry).name"
                        @click="emit('open', entry.resource)"
                      >
                        {{ historyLabel(entry).name || '未命名' }}
                      </button>
                      <p :title="historyLabel(entry).subtitle">
                        {{ historyLabel(entry).subtitle || '未知创作者'
                        }}<span v-if="!historyLabel(entry).available"> · 暂不可播放</span>
                      </p>
                    </div>
                  </div>
                  <div class="history-time" role="cell" :title="historyTime(entry.playedAt).full">
                    <span>{{ historyTime(entry.playedAt).short }}</span
                    ><small v-if="entry.playProgress !== null">{{
                      entry.listened
                        ? '已听完'
                        : `已听 ${Math.floor(entry.playProgress / 60)} 分 ${Math.floor(entry.playProgress % 60)} 秒`
                    }}</small>
                  </div>
                  <span class="history-device" role="cell">{{ entry.device }}</span>
                  <div class="history-actions" role="cell" @dblclick.stop>
                    <button
                      class="icon-button"
                      :aria-label="`播放 ${historyLabel(entry).name}`"
                      :disabled="!playable || loading || busy || !historyLabel(entry).available"
                      @click.stop="play(entry)"
                    >
                      <Icon name="play" /></button
                    ><button
                      class="icon-button"
                      :aria-label="`删除 ${historyLabel(entry).name} 的播放记录`"
                      :disabled="!canMutate"
                      @click.stop="remove({ kind: 'delete', ids: [entry.id] })"
                    >
                      <Icon name="close" />
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
        <p v-else-if="!loading && page" class="empty-state">
          {{
            keyword
              ? '没有匹配的播放记录'
              : error
                ? '当前没有可显示的缓存记录'
                : '这里还没有播放记录'
          }}
        </p>
      </div>
    </Transition>
    <p class="history-note muted">
      {{ page?.remoteIncluded ? '本机与多端记录已合并，按最近播放排序' : '显示本机播放记录'
      }}<template v-if="page?.remoteIncluded && page.remoteFetchedAt">
        · 最近读取 {{ historyTime(page.remoteFetchedAt).short }}</template
      >
    </p>
    <ModalTransition
      ><HistoryConfirm
        v-if="confirm"
        :title="confirm.title"
        :description="confirm.description"
        :busy="busy"
        :error="error"
        @close="confirm = undefined"
        @confirm="submit"
    /></ModalTransition>
  </section>
</template>
