<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useId } from 'vue';
import { useCore } from '../../services/core';
import type { AccountView } from '../../services/contracts';
import { messageTabs, type MessageRow, type MessageTab } from '../../services/messages';
import { useMessageDrawer } from '../../composables/useMessageDrawer';
import Icon from '../Icon.vue';

const props = defineProps<{ account: AccountView; initializing: boolean }>();
const emit = defineEmits<{ login: []; expired: [] }>();
const model = useMessageDrawer(
  useCore(),
  () => props.account,
  () => emit('expired'),
);
const { shown, tab, peer, current, counts, countsBusy, countsError } = model;
const trigger = ref<HTMLButtonElement>();
const drawer = ref<HTMLElement>();
const scroller = ref<HTMLElement>();
const backButton = ref<HTMLButtonElement>();
const uid = useId();
const labels: Record<MessageTab, string> = {
  private: '私信',
  comments: '评论',
  forwards: '@我',
  notices: '通知',
};
const empty: Record<MessageTab, string> = {
  private: '还没有私信',
  comments: '还没有收到评论',
  forwards: '还没有人提到你',
  notices: '还没有通知',
};
const showRows = computed(() =>
  peer.value ? [...current.value.rows].reverse() : current.value.rows,
);
const scrollPositions = new Map<MessageTab, number>();
let previousFocus: HTMLElement | null = null;
const date = new Intl.DateTimeFormat('zh-CN', {
  month: 'numeric',
  day: 'numeric',
  hour: '2-digit',
  minute: '2-digit',
});
function timestamp(value: number) {
  return value <= 8_640_000_000_000_000 ? date.format(value) : '';
}
function close(restore = true) {
  model.close();
  scrollPositions.clear();
  previousFocus = null;
  if (restore) trigger.value?.focus({ preventScroll: true });
}
async function toggle() {
  if (shown.value) return close();
  if (props.account.status !== 'authenticated') return emit('login');
  scrollPositions.clear();
  model.open();
  await nextTick();
  drawer.value?.querySelector<HTMLButtonElement>('[aria-selected="true"]')?.focus();
}
function outside(event: PointerEvent) {
  if (
    shown.value &&
    event.target instanceof Node &&
    !drawer.value?.contains(event.target) &&
    !trigger.value?.contains(event.target)
  )
    close(false);
}
onMounted(() => document.addEventListener('pointerdown', outside, true));
onBeforeUnmount(() => document.removeEventListener('pointerdown', outside, true));

async function select(next: MessageTab) {
  const list = scroller.value;
  if (tab.value === next) {
    list?.scrollTo(0, 0);
    return;
  }
  if (list) scrollPositions.set(tab.value, list.scrollTop);
  model.select(next);
  await nextTick();
  scroller.value?.scrollTo(0, scrollPositions.get(next) ?? 0);
}
async function moveTab(event: KeyboardEvent) {
  const index = messageTabs.indexOf(tab.value);
  const next = {
    ArrowRight: messageTabs[(index + 1) % messageTabs.length],
    ArrowLeft: messageTabs[(index + messageTabs.length - 1) % messageTabs.length],
    Home: messageTabs[0],
    End: messageTabs[messageTabs.length - 1],
  }[event.key];
  if (!next) return;
  event.preventDefault();
  await select(next);
  drawer.value?.querySelector<HTMLButtonElement>('[aria-selected="true"]')?.focus();
}
async function enter(row: MessageRow, event: MouseEvent) {
  if (!row.peerId) return;
  if (scroller.value) scrollPositions.set('private', scroller.value.scrollTop);
  previousFocus = event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
  const request = model.enter(row);
  await nextTick();
  backButton.value?.focus({ preventScroll: true });
  if (!(await request) || peer.value?.id !== row.peerId) return;
  await nextTick();
  if (scroller.value) scroller.value.scrollTop = scroller.value.scrollHeight;
}
async function back() {
  const previous = peer.value?.id;
  model.back();
  await nextTick();
  if (!shown.value || tab.value !== 'private' || peer.value) return;
  scroller.value?.scrollTo(0, scrollPositions.get('private') ?? 0);
  const row = [
    ...(scroller.value?.querySelectorAll<HTMLElement>('[data-message-peer]') ?? []),
  ].find((element) => element.dataset.messagePeer === previous);
  (previousFocus?.isConnected ? previousFocus : row)?.focus({ preventScroll: true });
}
async function load(mode: 'first' | 'more') {
  const list = scroller.value;
  const target = current.value;
  const contact = peer.value?.id;
  const height = list?.scrollHeight ?? 0;
  const top = list?.scrollTop ?? 0;
  if (!(await model.load(mode))) return;
  await nextTick();
  if (list !== scroller.value || target !== current.value || !list) return;
  if (contact)
    list.scrollTop = mode === 'more' ? list.scrollHeight - height + top : list.scrollHeight;
  else if (mode === 'first') list.scrollTop = 0;
}
function scroll() {
  const list = scroller.value;
  if (!list || current.value.error || current.value.busy) return;
  if (
    peer.value ? list.scrollTop < 60 : list.scrollHeight - list.clientHeight - list.scrollTop < 60
  )
    void load('more');
}
function refresh() {
  void model.readCounts();
  void load('first');
}
</script>

<template>
  <button
    ref="trigger"
    class="icon-button message-drawer-trigger"
    aria-label="消息"
    title="消息"
    aria-haspopup="dialog"
    :aria-expanded="shown"
    :disabled="initializing"
    @click="toggle"
  >
    <Icon name="mail" />
  </button>
  <Teleport to="body">
    <section
      v-if="shown"
      :id="`${uid}-messages`"
      ref="drawer"
      class="message-drawer"
      role="dialog"
      :aria-label="peer ? `与 ${peer.name} 的私信` : '消息'"
      @keydown.esc.stop.prevent="close()"
    >
      <header class="message-drawer-header">
        <button
          v-if="peer"
          ref="backButton"
          class="icon-button"
          aria-label="返回私信列表"
          @click="back"
        >
          <Icon name="back" />
        </button>
        <h2>{{ peer ? peer.name : '消息' }}</h2>
        <button
          class="icon-button"
          aria-label="刷新消息"
          title="刷新消息"
          :disabled="current.busy || countsBusy"
          @click="refresh"
        >
          <Icon name="refresh" />
        </button>
        <button class="icon-button" aria-label="关闭消息" title="关闭" @click="close()">
          <Icon name="close" />
        </button>
      </header>
      <nav v-if="!peer" class="message-drawer-tabs" role="tablist" aria-label="消息分类">
        <button
          v-for="option in messageTabs"
          :id="`${uid}-${option}`"
          :key="option"
          role="tab"
          :data-message-tab="option"
          :aria-selected="tab === option"
          :aria-controls="`${uid}-content`"
          :tabindex="tab === option ? 0 : -1"
          @click="select(option)"
          @keydown="moveTab"
        >
          {{ labels[option] }}
          <span
            v-if="counts?.[option]"
            class="message-drawer-badge"
            :title="`${counts[option]} 条未读`"
          >
            {{ counts[option]! > 99 ? '99+' : counts[option] }}
          </span>
        </button>
      </nav>
      <p v-if="countsError && !peer" class="message-drawer-count-error">
        未读数暂不可用
        <button @click="model.readCounts">重试</button>
      </p>
      <div
        :id="`${uid}-content`"
        ref="scroller"
        class="message-drawer-scroll"
        :role="peer ? undefined : 'tabpanel'"
        :aria-labelledby="peer ? undefined : `${uid}-${tab}`"
        :aria-busy="current.busy"
        @scroll.passive="scroll"
      >
        <p v-if="peer && current.hint" class="message-drawer-status">{{ current.hint }}</p>
        <div
          v-if="peer && current.loaded && current.rows.length && (current.cursor || current.capped)"
          class="message-drawer-status"
        >
          <button
            v-if="!current.capped && !current.error"
            :disabled="current.busy"
            @click="load('more')"
          >
            查看更早消息
          </button>
          <span v-else-if="current.capped">本次最多显示 300 条</span>
          <button v-else @click="load(current.errorMode)">重试读取更早消息</button>
        </div>
        <ul v-if="showRows.length" class="message-drawer-list">
          <li
            v-for="item in showRows"
            :key="item.id"
            class="message-drawer-item"
            :class="{ 'message-drawer-own': peer && item.own }"
          >
            <button
              v-if="!peer && tab === 'private' && item.peerId"
              class="message-conversation"
              :data-message-peer="item.peerId"
              @click="enter(item, $event)"
            >
              <span class="message-drawer-avatar" aria-hidden="true">{{
                item.author.slice(0, 1)
              }}</span>
              <span class="message-drawer-copy">
                <span class="message-drawer-name">{{ item.author }}</span>
                <span class="message-drawer-preview"
                  >{{ item.caption ? `${item.caption}：` : '' }}{{ item.body }}</span
                >
              </span>
              <span class="message-drawer-meta">
                <time v-if="item.time">{{ timestamp(item.time) }}</time>
                <span v-if="item.unread" class="message-drawer-badge">{{
                  item.unread > 99 ? '99+' : item.unread
                }}</span>
              </span>
            </button>
            <article v-else class="message-drawer-article">
              <span class="message-drawer-avatar" aria-hidden="true">{{
                item.author.slice(0, 1)
              }}</span>
              <div class="message-drawer-copy">
                <header>
                  <strong class="message-drawer-name">{{ item.author }}</strong>
                  <time v-if="item.time">{{ timestamp(item.time) }}</time>
                </header>
                <p v-if="item.caption" class="message-drawer-caption">{{ item.caption }}</p>
                <p class="message-drawer-text">{{ item.body }}</p>
                <blockquote v-if="item.quote">{{ item.quote }}</blockquote>
              </div>
            </article>
          </li>
        </ul>
        <p v-if="current.busy && !current.rows.length" class="message-drawer-status" role="status">
          正在读取消息…
        </p>
        <p
          v-else-if="
            current.error && (!peer || !current.rows.length || current.errorMode === 'first')
          "
          class="message-drawer-status"
          role="status"
        >
          {{ current.errorMode === 'more' ? '更早的消息读取失败' : '消息暂时无法读取' }}
          <button @click="load(current.errorMode)">重试</button>
        </p>
        <p v-else-if="current.loaded && !current.rows.length" class="message-drawer-status">
          {{ peer ? '这段会话还没有消息' : empty[tab] }}
        </p>
        <div
          v-else-if="
            !peer && current.loaded && current.rows.length && (current.cursor || current.capped)
          "
          class="message-drawer-status"
        >
          <button v-if="!current.capped" :disabled="current.busy" @click="load('more')">
            加载更多
          </button>
          <span v-else>本次最多显示 300 条</span>
        </div>
      </div>
    </section>
  </Teleport>
</template>

<style src="./message-drawer.css"></style>
