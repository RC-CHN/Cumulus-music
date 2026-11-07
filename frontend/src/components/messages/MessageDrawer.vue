<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, useId, watch } from 'vue';
import { useCore } from '../../services/core';
import type { AccountView } from '../../services/contracts';
import {
  messageTabs,
  type MessageDeleteOutcome,
  type MessagePendingDeletion,
  type MessageRow,
  type MessageTab,
  type MessageTarget,
} from '../../services/messages';
import { useMessageDrawer } from '../../composables/useMessageDrawer';
import { useMessageReadAll } from '../../composables/useMessageReadAll';
import ActionMenu from '../ActionMenu.vue';
import Icon from '../Icon.vue';
import ModalTransition from '../ModalTransition.vue';
import MessageDeleteDialog from './MessageDeleteDialog.vue';
import MessageReadAllDialog from './MessageReadAllDialog.vue';
import PrivateReplyComposer from './PrivateReplyComposer.vue';

const props = defineProps<{
  account: AccountView;
  initializing: boolean;
  route: object;
  playable: boolean;
  openTarget: (epoch: string, target: MessageTarget) => object | undefined;
}>();
const emit = defineEmits<{ login: []; expired: [] }>();
const core = useCore();
const model = useMessageDrawer(
  core,
  () => props.account,
  () => emit('expired'),
);
const { shown, tab, peer, current, counts, countsBusy, countsError } = model;
const trigger = ref<HTMLButtonElement>();
const drawer = ref<HTMLElement>();
const scroller = ref<HTMLElement>();
const backButton = ref<HTMLButtonElement>();
const readAllButton = ref<HTMLButtonElement>();
const uid = useId();
const deleteMenuLabel = '私信操作';
const replyBusy = ref(false);
const replyDrafts = ref(new Map<string, string>());
const replyUncertain = ref(new Map<string, string>());
function setReplyDraft(scope: object, value: string) {
  if (current.value !== scope) return;
  const id = peer.value?.id;
  if (!id) return;
  replyDrafts.value.delete(id);
  if (value) replyDrafts.value.set(id, value);
  if (replyDrafts.value.size > 128) {
    const oldest = replyDrafts.value.keys().next().value;
    if (oldest) replyDrafts.value.delete(oldest);
  }
}
function setReplyUncertain(scope: object, value?: string) {
  if (!peer.value || current.value !== scope) return;
  if (value) replyUncertain.value.set(peer.value.id, value);
  else replyUncertain.value.delete(peer.value.id);
}
function setReplyReceipt(scope: object, value: string) {
  if (peer.value && current.value === scope) current.value.replyReceipt = value;
}
function setReplyBusy(scope: object, value: boolean) {
  if (peer.value && current.value === scope) replyBusy.value = value;
}
function replyExpired(scope: object) {
  if (peer.value && current.value === scope) emit('expired');
}
async function deliveredReply(scope: object, row: MessageRow) {
  if (!peer.value || !row.own || current.value !== scope) return;
  const list = current.value;
  list.rows = [row, ...list.rows.filter((item) => item.id !== row.id)].slice(0, 300);
  await nextTick();
  if (current.value === list && scroller.value)
    scroller.value.scrollTop = scroller.value.scrollHeight;
}
function resetReply() {
  replyDrafts.value.clear();
  replyUncertain.value.clear();
  replyBusy.value = false;
}
const deleteMenu = ref<InstanceType<typeof ActionMenu>>();
const deleteBusy = ref(false);
const deleteNotice = ref('');
const pendingDeletes = ref<MessagePendingDeletion[]>([]);
const pendingReadFailed = ref(false);
const deleteMenuTarget = shallowRef<{
  row: MessageRow;
  epoch: string;
  anchor: HTMLElement;
}>();
const confirmation = shallowRef<typeof deleteMenuTarget.value>();
let deletionVisit = 0;
let pendingRead = 0;
const deleteOptions = computed(() =>
  deleteMenuTarget.value?.row.deletionPending
    ? [{ key: 'check', label: '核对删除结果', icon: 'refresh' }]
    : [
        {
          key: 'remove',
          label: peer.value ? '删除这条私信' : '删除私信',
          icon: 'trash',
          danger: true,
        },
      ],
);
const unmatchedDeletes = computed(() => {
  if (peer.value || tab.value !== 'private') return [];
  const visibleReceipts = new Set(current.value.rows.map((row) => row.deletionReceipt));
  return pendingDeletes.value.filter((operation) => !visibleReceipts.has(operation.receipt));
});
const deleteResultText: Record<MessageDeleteOutcome, string> = {
  removed: '删除成功。',
  uncertain: '删除结果尚未确认，已保留原消息。可以核对结果。',
  still_listed: '当前页仍能看到这项私信，先前的删除结果尚未确认。',
  not_in_page: '当前页未看到这项私信，无法确认先前的删除是否成功。',
  read_unavailable: '暂时无法核对删除结果，已保留待核对记录。',
};

function resetDeletion() {
  deletionVisit++;
  pendingRead++;
  deleteMenu.value?.close();
  deleteMenuTarget.value = undefined;
  confirmation.value = undefined;
  deleteBusy.value = false;
  deleteNotice.value = '';
  pendingDeletes.value = [];
  pendingReadFailed.value = false;
}
async function readPending() {
  const epoch = props.account.epoch;
  const sequence = ++pendingRead;
  const isCurrent = () => sequence === pendingRead && shown.value && props.account.epoch === epoch;
  try {
    const result = await core.pendingMessageDeletions(epoch);
    if (!isCurrent() || result.epoch !== epoch) return;
    pendingReadFailed.value = false;
    pendingDeletes.value = result.value;
  } catch {
    if (isCurrent()) pendingReadFailed.value = true;
  }
}
function openDeleteMenu(item: MessageRow, event: MouseEvent | KeyboardEvent) {
  if (
    !item.deletionReceipt ||
    !visible.value ||
    current.value.busy ||
    deleteBusy.value ||
    replyBusy.value ||
    confirmation.value
  )
    return;
  if (
    event instanceof KeyboardEvent &&
    !(event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey))
  )
    return;
  const owner = event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
  const anchor = owner?.matches('[data-message-delete]')
    ? owner
    : owner?.querySelector<HTMLElement>('[data-message-delete]');
  if (!anchor) return;
  event.preventDefault();
  event.stopPropagation();
  deleteMenuTarget.value = { row: item, epoch: props.account.epoch, anchor };
  const bounds = anchor.getBoundingClientRect();
  const rightClick = event instanceof MouseEvent && event.type === 'contextmenu';
  void deleteMenu.value?.showAt(
    rightClick && event.clientX ? event.clientX : bounds.left,
    rightClick && event.clientY ? event.clientY : bounds.bottom,
    anchor,
  );
}
function chooseDeletion(action: string) {
  const chosen = deleteMenuTarget.value;
  if (!chosen || chosen.epoch !== props.account.epoch || !current.value.rows.includes(chosen.row))
    return;
  if (action === 'check' && chosen.row.deletionReceipt) {
    void settleDeletion(chosen.row.deletionReceipt, 'check', chosen.row);
  } else if (action === 'remove' && !chosen.row.deletionPending) {
    confirmation.value = chosen;
  }
}
function confirmDeletion() {
  const chosen = confirmation.value;
  if (
    chosen?.row.deletionReceipt &&
    chosen.epoch === props.account.epoch &&
    current.value.rows.includes(chosen.row)
  )
    void settleDeletion(chosen.row.deletionReceipt, 'remove', chosen.row);
}
function deletionFallback() {
  return (
    backButton.value ?? drawer.value?.querySelector<HTMLElement>('[aria-selected="true"]') ?? null
  );
}
async function settleDeletion(receipt: string, action: 'remove' | 'check', row?: MessageRow) {
  if (!visible.value || deleteBusy.value) return;
  const epoch = props.account.epoch;
  const sequence = deletionVisit;
  const list = current.value;
  const isCurrent = () =>
    sequence === deletionVisit && shown.value && props.account.epoch === epoch;
  deleteBusy.value = true;
  deleteNotice.value = '';
  try {
    const result = await core.deleteMessage(epoch, receipt, action);
    if (!isCurrent() || result.epoch !== epoch) return;
    if (result.value === 'removed' && row) {
      list.rows = list.rows.filter((item) => item.deletionReceipt !== receipt);
    } else if (result.value === 'uncertain' && row) {
      row.deletionPending = true;
      pendingDeletes.value = pendingDeletes.value.some((item) => item.receipt === receipt)
        ? pendingDeletes.value
        : [...pendingDeletes.value, { receipt, conversation: !peer.value }];
    }
    deleteNotice.value = deleteResultText[result.value];
  } catch (error) {
    if (!isCurrent()) return;
    const code = (error as { code?: string })?.code;
    if (code === 'session_expired') emit('expired');
    deleteNotice.value = '未能完成操作，已保留原消息。';
  } finally {
    if (isCurrent()) {
      confirmation.value = undefined;
      deleteBusy.value = false;
      await readPending();
      if (isCurrent() && row && pendingDeletes.value.some((item) => item.receipt === receipt))
        row.deletionPending = true;
    }
  }
}
function drawerEscape() {
  if (!confirmation.value && !deleteBusy.value && !readAll.confirmation.value) close();
}
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
const suspended = shallowRef<{
  epoch: string;
  origin: object;
  destination: object;
  row: string;
  top: number;
}>();
const visible = computed(() => shown.value && !suspended.value);
const unread = computed(() => messageTabs.some((option) => (counts.value?.[option] ?? 0) > 0));
const readAll = useMessageReadAll(
  core,
  () =>
    visible.value && props.account.status === 'authenticated' ? props.account.epoch : undefined,
  model.clearConversationBadges,
  () => emit('expired'),
);
const readAllDisabled = computed(
  () =>
    !unread.value ||
    countsBusy.value ||
    countsError.value ||
    readAll.busy.value ||
    readAll.blocked.value ||
    deleteBusy.value ||
    replyBusy.value ||
    !!confirmation.value,
);
function prepareReadAll() {
  if (!readAllDisabled.value) readAll.prepare();
}
function readAllReturn() {
  return readAllButton.value ?? null;
}
function reportCountVisibility() {
  model.setCountVisibility(visible.value && document.visibilityState === 'visible');
}
watch(visible, reportCountVisibility, { flush: 'post' });
let restoringScroll: HTMLElement | undefined;
const attachmentLabels: Record<MessageTarget['kind'], string> = {
  play_track: '播放单曲',
  play_voice: '播放声音',
  playlist: '查看歌单',
  album: '查看专辑',
  artist: '查看艺人',
  mv: '打开 MV',
  comment: '查看这条评论',
};
function playTarget(target: MessageTarget) {
  return target.kind === 'play_track' || target.kind === 'play_voice';
}
function activateAttachment(row: MessageRow) {
  const target = row.target;
  if (!visible.value || !target || !current.value.rows.includes(row)) return;
  if (playTarget(target) && !props.playable) return;
  const epoch = props.account.epoch;
  const origin = props.route;
  const top = scroller.value?.scrollTop ?? 0;
  const destination = props.openTarget(epoch, target);
  if (destination && destination !== origin)
    suspended.value = { epoch, origin, destination, row: row.id, top };
}
watch(
  () => props.route,
  async (route) => {
    const saved = suspended.value;
    if (!saved || route === saved.destination) return;
    if (route !== saved.origin || props.account.epoch !== saved.epoch) {
      close(false);
      return;
    }
    const previous = document.activeElement;
    suspended.value = undefined;
    await nextTick();
    if (!visible.value || props.route !== saved.origin || props.account.epoch !== saved.epoch)
      return;
    const list = scroller.value;
    if (list) {
      restoringScroll = list;
      list.scrollTo(0, saved.top);
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          if (restoringScroll === list) restoringScroll = undefined;
        }),
      );
    }
    if (document.activeElement !== previous && document.activeElement !== document.body) return;
    const anchor = [
      ...(scroller.value?.querySelectorAll<HTMLButtonElement>('[data-message-attachment]') ?? []),
    ].find((button) => button.dataset.messageAttachment === saved.row);
    anchor?.focus({ preventScroll: true });
  },
);
watch(
  () => [props.account.epoch, props.account.status],
  () => {
    suspended.value = undefined;
    resetDeletion();
    resetReply();
  },
  { flush: 'sync' },
);
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
  suspended.value = undefined;
  model.close();
  resetDeletion();
  resetReply();
  scrollPositions.clear();
  previousFocus = null;
  if (restore) trigger.value?.focus({ preventScroll: true });
}
async function toggle() {
  if (visible.value) return close();
  if (suspended.value) close(false);
  if (props.account.status !== 'authenticated') return emit('login');
  scrollPositions.clear();
  model.open();
  void readPending();
  await nextTick();
  drawer.value?.querySelector<HTMLButtonElement>('[aria-selected="true"]')?.focus();
}
function outside(event: PointerEvent) {
  if (
    visible.value &&
    event.target instanceof Element &&
    !confirmation.value &&
    !readAll.confirmation.value &&
    !deleteBusy.value &&
    !event.target.closest(`[role="menu"][aria-label="${deleteMenuLabel}"]`) &&
    !drawer.value?.contains(event.target) &&
    !trigger.value?.contains(event.target)
  )
    close(false);
}
onMounted(() => {
  document.addEventListener('pointerdown', outside, true);
  document.addEventListener('visibilitychange', reportCountVisibility);
});
onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', outside, true);
  document.removeEventListener('visibilitychange', reportCountVisibility);
});

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
  replyBusy.value = false;
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
  if (
    !list ||
    list === restoringScroll ||
    current.value.error ||
    current.value.busy ||
    confirmation.value ||
    readAll.confirmation.value ||
    deleteBusy.value ||
    replyBusy.value
  )
    return;
  // Switching views can clamp the previous scroll position to zero, which
  // is not a user scroll to the edge. Short lists use their explicit buttons.
  const range = list.scrollHeight - list.clientHeight;
  if (range > 60 && (peer.value ? list.scrollTop < 60 : range - list.scrollTop < 60))
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
    :aria-description="unread ? '有未读消息' : undefined"
    aria-haspopup="dialog"
    :aria-expanded="visible"
    :disabled="initializing"
    @click="toggle"
  >
    <Icon name="mail" />
    <span v-if="unread" class="message-drawer-unread-dot" aria-hidden="true" />
  </button>
  <Teleport to="body">
    <section
      v-if="visible"
      :id="`${uid}-messages`"
      ref="drawer"
      class="message-drawer"
      role="dialog"
      :aria-label="peer ? `与 ${peer.name} 的私信` : '消息'"
      @keydown.esc.stop.prevent="drawerEscape"
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
          :disabled="current.busy || countsBusy || deleteBusy || replyBusy"
          @click="refresh"
        >
          <Icon name="refresh" />
        </button>
        <button class="icon-button" aria-label="关闭消息" title="关闭" @click="close()">
          <Icon name="close" />
        </button>
      </header>
      <div v-if="!peer" class="message-drawer-classification">
        <nav class="message-drawer-tabs" role="tablist" aria-label="消息分类">
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
        <button
          ref="readAllButton"
          class="message-drawer-read-all"
          type="button"
          aria-label="一键全读"
          aria-haspopup="dialog"
          :disabled="readAllDisabled"
          @click="prepareReadAll"
        >
          <Icon name="check" /> 一键全读
        </button>
      </div>
      <p v-if="countsError && !peer" class="message-drawer-count-error">
        未读数暂不可用
        <button @click="model.readCounts">重试</button>
      </p>
      <p
        v-if="!peer && readAll.notice.value"
        class="message-drawer-status message-drawer-read-all-status"
        role="status"
        data-message-read-all-status
      >
        {{ readAll.notice.value }}
        <button
          v-if="readAll.canRecover.value"
          type="button"
          :disabled="readAll.busy.value"
          @click="readAll.recover"
        >
          {{ readAll.readProgress.value ? '查看进度' : '核对未读数' }}
        </button>
      </p>
      <p v-if="deleteNotice" class="message-drawer-status" role="status" data-message-delete-result>
        {{ deleteNotice }}
      </p>
      <p v-if="pendingReadFailed && !peer && tab === 'private'" class="message-drawer-count-error">
        暂时无法读取待核对的删除记录 <button @click="readPending">重试</button>
      </p>
      <div v-if="unmatchedDeletes.length" class="message-drawer-pending">
        <p>还有 {{ unmatchedDeletes.length }} 项待核对的删除记录</p>
        <ul aria-label="待核对的私信删除">
          <li v-for="operation in unmatchedDeletes" :key="operation.receipt">
            <button
              type="button"
              data-message-check-pending
              :disabled="deleteBusy"
              @click="settleDeletion(operation.receipt, 'check')"
            >
              核对{{ operation.conversation ? '私信会话' : '单条私信' }}删除结果
            </button>
          </li>
        </ul>
      </div>
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
            :class="{
              'message-drawer-own': peer && item.own,
              'message-drawer-editable': item.deletionReceipt,
            }"
            @contextmenu="openDeleteMenu(item, $event)"
            @keydown="openDeleteMenu(item, $event)"
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
                <button
                  v-if="item.target"
                  class="message-drawer-attachment"
                  :data-message-attachment="item.id"
                  :data-target-kind="item.target.kind"
                  :disabled="playTarget(item.target) && !playable"
                  @click="activateAttachment(item)"
                >
                  <Icon :name="playTarget(item.target) ? 'play' : 'forward'" />
                  {{ attachmentLabels[item.target.kind] }}
                </button>
              </div>
            </article>
            <button
              v-if="item.deletionReceipt"
              type="button"
              class="icon-button message-delete-action"
              :data-message-delete="item.id"
              aria-haspopup="menu"
              aria-expanded="false"
              :aria-label="
                item.deletionPending
                  ? '待核对的私信删除'
                  : peer
                    ? '这条私信的操作'
                    : `与 ${item.author} 的私信操作`
              "
              :title="item.deletionPending ? '删除结果待核对' : '私信操作'"
              :disabled="deleteBusy"
              @click="openDeleteMenu(item, $event)"
            >
              <Icon :name="item.deletionPending ? 'refresh' : 'more'" />
            </button>
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
      <PrivateReplyComposer
        v-if="peer && current.replyReceipt"
        :key="peer.id"
        :epoch="account.epoch"
        :scope="current"
        :receipt="current.replyReceipt"
        :draft="replyDrafts.get(peer.id) ?? ''"
        :uncertain-attempt="replyUncertain.get(peer.id)"
        :blocked="current.busy || deleteBusy || !!confirmation"
        @draft="setReplyDraft"
        @receipt="setReplyReceipt"
        @uncertain-attempt="setReplyUncertain"
        @delivered="deliveredReply"
        @busy="setReplyBusy"
        @expired="replyExpired"
      />
      <ActionMenu
        ref="deleteMenu"
        :label="deleteMenuLabel"
        :items="deleteOptions"
        :disabled="deleteBusy || replyBusy || !!confirmation"
        variant="song"
        context-only
        @select="chooseDeletion"
      />
      <ModalTransition :fallback-focus="deletionFallback">
        <MessageDeleteDialog
          v-if="confirmation"
          :conversation="!peer"
          :busy="deleteBusy"
          @close="confirmation = undefined"
          @confirm="confirmDeletion"
        />
      </ModalTransition>
      <ModalTransition :return-focus="readAllReturn" :fallback-focus="deletionFallback">
        <MessageReadAllDialog
          v-if="readAll.confirmation.value"
          :busy="readAll.busy.value"
          @close="readAll.cancel"
          @confirm="readAll.confirm"
        />
      </ModalTransition>
    </section>
  </Teleport>
</template>

<style src="./message-drawer.css"></style>
