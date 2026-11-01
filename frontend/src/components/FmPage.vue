<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import type { FmModel } from '../composables/useFm';
import type { PlayerModel } from '../composables/usePlayer';
import type { TrackView } from '../services/contracts';
import Icon from './Icon.vue';
import FmExplore from './FmExplore.vue';
import type { FmCard, FmFeedback } from '../services/fm';
import { fmPrimary } from '../services/fmPrimary';
import { artistLabel } from '../services/artistLabel';
import ModalTransition from './ModalTransition.vue';
import FmFeedbackDialog from './fm/FmFeedbackDialog.vue';
import { useFmFeedback } from '../composables/useFmFeedback';
import FmStateRecovery from './fm/FmStateRecovery.vue';
import FmVirtualList from './fm/FmVirtualList.vue';
const props = defineProps<{
  model: FmModel;
  player: PlayerModel;
  liked: Set<string>;
  likeBusy: Set<string>;
}>();
const emit = defineEmits<{
  like: [track: TrackView];
  comments: [track: TrackView];
  download: [tracks: TrackView[]];
}>();
const { view, trash, reasons, reasonsLoading, loading, busy, trashLoading, error } = props.model;
const tab = ref<'fm' | 'trash'>('fm'),
  selected = ref(new Set<string>());
let restoreIds = new Set<string>();
const exploring = ref(false);
const heading = ref<HTMLHeadingElement>(),
  returnButton = ref<HTMLButtonElement>(),
  feedbackButton = ref<HTMLButtonElement>(),
  trashToolbar = ref<HTMLElement>(),
  stateRegion = ref<HTMLElement>(),
  recovery = ref<HTMLElement>();
const feedback = useFmFeedback(props.model, () => props.player.view.value);
const afterFeedback = ref<'trash' | 'recovery'>();
async function leave() {
  const owner = returnButton.value,
    focused = document.activeElement === owner;
  const epoch = props.player.view.value?.epoch;
  await props.model.command({ kind: 'leave' });
  await nextTick();
  if (
    focused &&
    heading.value?.isConnected &&
    epoch === props.player.view.value?.epoch &&
    props.model.view.value?.active === false &&
    props.model.view.value.activeSource === null &&
    (document.activeElement === document.body || document.activeElement === owner)
  )
    heading.value.focus({ preventScroll: true });
}
function explore() {
  exploring.value = !exploring.value;
  if (exploring.value) void props.model.discovery.load({ kind: 'home' });
}
async function selectCard(card: FmCard, autoplay: boolean) {
  exploring.value = false;
  const selected = await props.model.select(card.source, card.title);
  if (
    selected &&
    autoplay &&
    !props.model.error.value &&
    props.model.view.value &&
    JSON.stringify(props.model.source.value) === JSON.stringify(card.source)
  ) {
    await props.model.command({
      kind: 'start',
      quality: props.player.view.value?.qualityPreferences.quality ?? 'standard',
    });
  }
}
const current = computed(() => view.value?.items.find((i) => i.entryId === view.value?.current));
const pending = computed(
  () =>
    busy.value ||
    !!props.model.stateIssue.value ||
    props.player.busy.value ||
    !props.player.ready.value,
);
const writeLocked = computed(() => busy.value || !!props.model.stateIssue.value);
const listLocked = computed(
  () =>
    writeLocked.value ||
    trashLoading.value ||
    (!!props.model.trashIssue.value && !props.model.trashIssue.value.more),
);
// Empty recommendations can still own the saved ordinary queue and its return
// action. Transport controls additionally require the matching FM projection
// and a current item; an empty FM projection still owns its repeat policy.
const active = computed(() => !!view.value?.active);
const transportActive = computed(() => active.value && props.player.view.value?.fm === true);
const playing = computed(() => transportActive.value && props.player.playing.value);
const modePending = computed(() => active.value && !!view.value?.modePending);
const restorePending = computed(
  () => !!view.value?.operations.some((op) => op.kind === 'trash' && op.desired === false),
);
const primary = computed(() =>
  fmPrimary({
    active: active.value,
    playing: playing.value,
    modePending: modePending.value,
    transportActive: transportActive.value,
    hasCurrent: !!current.value,
    pending: !!view.value?.pending,
    notice: !!view.value?.notice,
  }),
);
watch([() => props.player.view.value?.epoch, () => JSON.stringify(view.value?.source)], () => {
  selected.value = new Set();
  restoreIds = new Set();
});
function toggle(id: string) {
  const next = new Set(selected.value);
  if (next.has(id)) next.delete(id);
  else if (next.size < 100) next.add(id);
  selected.value = next;
}
function openTrash() {
  tab.value = 'trash';
  void props.model.loadTrash();
}
function removeSelection(ids: Set<string>) {
  selected.value = new Set([...selected.value].filter((id) => !ids.has(id)));
}
async function readTrash(event: Event, more = false) {
  const trigger = event.currentTarget,
    owner = props.player.view.value?.epoch,
    channel = JSON.stringify(props.model.source.value),
    owned = trigger instanceof HTMLElement && document.activeElement === trigger;
  if (owned) trashToolbar.value?.focus({ preventScroll: true });
  await props.model.loadTrash(more);
  await nextTick();
  if (
    owned &&
    tab.value === 'trash' &&
    owner === props.player.view.value?.epoch &&
    channel === JSON.stringify(props.model.source.value) &&
    [trashToolbar.value, document.body].includes(document.activeElement as HTMLElement)
  ) {
    const next =
      trigger.isConnected && !trigger.matches(':disabled')
        ? trigger
        : (trashToolbar.value?.parentElement?.querySelector<HTMLElement>('.fm-list-error button') ??
          trashToolbar.value);
    next?.focus({ preventScroll: true });
    next?.scrollIntoView({ block: 'nearest' });
  }
}
async function unblock(id: string, event: Event) {
  const trigger = event.currentTarget as HTMLElement,
    list = trigger.closest<HTMLElement>('.fm-virtual-list'),
    index = Number(trigger.closest<HTMLElement>('[data-fm-index]')?.dataset.fmIndex),
    owner = props.player.view.value?.epoch,
    owned = document.activeElement === trigger;
  if (owned) list?.focus({ preventScroll: true });
  await props.model.command({ kind: 'artist', id, blocked: false });
  await nextTick();
  if (
    owned &&
    list?.isConnected &&
    tab.value === 'trash' &&
    owner === props.player.view.value?.epoch &&
    [list, document.body].includes(document.activeElement as HTMLElement)
  ) {
    const next = props.model.stateIssue.value
      ? stateRegion.value
      : trigger.isConnected && !trigger.matches(':disabled')
        ? trigger
        : (list.querySelector<HTMLElement>(
            `[data-fm-index="${Math.min(index, (view.value?.blockedArtists.length ?? 1) - 1)}"] button:not(:disabled)`,
          ) ?? list);
    next?.focus({ preventScroll: true });
    next?.scrollIntoView({ block: 'nearest' });
  }
}
async function restore(event?: Event, ids = [...selected.value]) {
  if (restorePending.value || listLocked.value || !ids.length) return;
  const trigger = event?.currentTarget,
    owned = trigger instanceof HTMLElement && document.activeElement === trigger,
    owner = props.player.view.value?.epoch,
    channel = JSON.stringify(props.model.source.value);
  if (owned) trashToolbar.value?.focus({ preventScroll: true });
  const submitted = new Set(ids);
  restoreIds = submitted;
  const result = await props.model.command({ kind: 'restore_trash', ids });
  if (result?.ok) {
    removeSelection(submitted);
    restoreIds = new Set();
    await props.model.loadTrash();
  }
  await nextTick();
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
  if (
    owned &&
    tab.value === 'trash' &&
    owner === props.player.view.value?.epoch &&
    channel === JSON.stringify(props.model.source.value) &&
    (document.activeElement === trashToolbar.value || document.activeElement === document.body)
  ) {
    const next = props.model.stateIssue.value
      ? stateRegion.value
      : trigger.isConnected && !trigger.matches(':disabled')
        ? trigger
        : trashToolbar.value;
    next?.focus({ preventScroll: true });
    next?.scrollIntoView({ block: 'nearest' });
  }
}
async function readState(event: Event) {
  const trigger = event.currentTarget,
    owner = props.player.view.value?.epoch;
  const owned = trigger instanceof HTMLElement && document.activeElement === trigger;
  const parked = stateRegion.value;
  if (owned) parked?.focus({ preventScroll: true });
  await props.model.load(true);
  await nextTick();
  if (
    owned &&
    owner === props.player.view.value?.epoch &&
    (document.activeElement === parked || document.activeElement === document.body)
  ) {
    const target = trigger.isConnected && !trigger.matches(':disabled') ? trigger : heading.value;
    target?.focus({ preventScroll: true });
    target?.scrollIntoView({ block: 'nearest' });
  }
}
async function reconcileFeedback(op: FmFeedback, event: Event) {
  const trigger = event.currentTarget,
    owned = trigger instanceof HTMLElement && document.activeElement === trigger,
    owner = props.player.view.value?.epoch;
  if (owned) recovery.value?.focus({ preventScroll: true });
  const result = await props.model.command({ kind: 'reconcile', id: op.id });
  if (result?.ok && op.desired === false) {
    removeSelection(restoreIds);
    restoreIds = new Set();
    await props.model.loadTrash();
  }
  await nextTick();
  if (
    owned &&
    owner === props.player.view.value?.epoch &&
    (document.activeElement === recovery.value || document.activeElement === document.body)
  ) {
    const next =
      trigger.isConnected && !trigger.matches(':disabled')
        ? trigger
        : op.desired === false
          ? trashToolbar.value
          : (recovery.value ?? heading.value);
    next?.focus({ preventScroll: true });
    next?.scrollIntoView({ block: 'nearest' });
  }
}
function feedbackNavigate(destination: 'trash' | 'recovery') {
  if (feedback.busy.value) return;
  afterFeedback.value = destination;
  feedback.close();
}
async function feedbackLeft() {
  const destination = afterFeedback.value;
  afterFeedback.value = undefined;
  if (!destination || !feedback.accountCurrent.value) return;
  if (destination === 'trash') openTrash();
  await nextTick();
  const target = destination === 'recovery' ? recovery.value : heading.value;
  target?.focus({ preventScroll: true });
  target?.scrollIntoView({ block: 'nearest' });
}
function play() {
  switch (primary.value.action) {
    case 'resume':
      resume();
      break;
    case 'pause':
      void props.player.action({ kind: 'pause', paused: true });
      break;
    case 'play':
      void props.player.action({ kind: 'play' });
      break;
    case 'start':
      void props.model.command({
        kind: 'start',
        quality: props.player.view.value?.qualityPreferences.quality ?? 'standard',
      });
  }
}
function resume() {
  heading.value?.focus({ preventScroll: true });
  void props.model.command({ kind: 'resume' });
}
</script>
<template>
  <section
    class="fm-page"
    aria-label="私人FM"
    :data-fm-source="JSON.stringify(model.source.value)"
    :data-mode-pending="modePending"
    :data-fm-state-error="model.stateIssue.value?.code"
  >
    <header class="fm-header">
      <div>
        <h1 ref="heading" tabindex="-1">{{ tab === 'fm' ? model.title.value : '音乐黑名单' }}</h1>
        <p>
          {{
            tab === 'fm'
              ? '从喜欢的旋律，开启无限漫游'
              : '这里的歌曲将不再出现在私人漫游和个性化推荐中'
          }}
        </p>
      </div>
      <button
        v-if="tab === 'fm'"
        class="text-button"
        :disabled="pending"
        :aria-expanded="exploring"
        @click="explore"
      >
        {{ exploring ? '收起模式' : '切换漫游' }}
      </button>
      <button
        class="text-button"
        :disabled="busy"
        @click="tab === 'fm' ? openTrash() : (tab = 'fm')"
      >
        {{ tab === 'fm' ? '减少推荐列表' : `返回${model.title.value}` }}
      </button>
    </header>
    <FmExplore v-if="tab === 'fm' && exploring" :model="model" @select="selectCard" />
    <section
      v-if="model.stateIssue.value"
      ref="stateRegion"
      tabindex="-1"
      class="fm-state-region"
      aria-label="读取状态恢复"
    >
      <FmStateRecovery
        :issue="model.stateIssue.value.message"
        :previous="model.writeIssue.value?.message"
        :busy="loading || busy"
        @retry="readState"
      />
    </section>
    <div v-else-if="error" class="error-state" role="alert">
      {{ error
      }}<button @click="tab === 'fm' ? model.load(true) : model.loadTrash()">重新加载</button>
    </div>
    <div v-if="view?.pending || view?.notice" class="fm-notice" role="status">
      {{ view.notice || '切歌尚未完成'
      }}<button :disabled="pending" @click="resume">
        {{ modePending ? '同步并继续播放' : '重试继续播放' }}
      </button>
    </div>
    <div v-if="tab === 'fm'" class="fm-main">
      <div v-if="loading && !view" class="empty-state">正在寻找你的下一首歌…</div>
      <div v-else class="fm-now">
        <div class="fm-art">
          <img
            v-if="current?.track.coverUrl"
            :src="current.track.coverUrl"
            alt="当前歌曲封面"
            referrerpolicy="no-referrer"
          /><Icon v-else name="music" />
        </div>
        <div class="fm-copy">
          <span class="fm-label">{{ model.title.value }} · 无限发现好音乐</span>
          <h2>{{ current?.track.name ?? '暂时没有可播放的推荐' }}</h2>
          <p>{{ artistLabel(current?.track, '重新获取推荐，发现更多音乐') }}</p>
          <p class="fm-album">{{ current?.track.album }}</p>
          <div class="fm-actions">
            <button class="primary" :disabled="pending" @click="play">
              <Icon :name="primary.icon" />{{ primary.label }}
            </button>
            <button
              class="icon-button"
              aria-label="FM上一首"
              :disabled="pending || !transportActive || view?.pending || !current"
              @click="player.action({ kind: 'previous' })"
            >
              <Icon name="previous" />
            </button>
            <button
              class="icon-button"
              aria-label="FM下一首"
              :disabled="pending || !transportActive || view?.pending || !current"
              @click="player.action({ kind: 'next' })"
            >
              <Icon name="next" />
            </button>
          </div>
          <div v-if="current" class="fm-tools">
            <button
              class="text-button"
              :class="{ liked: liked.has(current.track.id) }"
              :disabled="likeBusy.has(current.track.id)"
              @click="emit('like', current.track)"
            >
              <Icon name="heart" />{{ liked.has(current.track.id) ? '已喜欢' : '喜欢' }}
            </button>
            <button
              ref="feedbackButton"
              class="text-button"
              :disabled="pending || !transportActive || view?.pending"
              @click="feedback.show"
            >
              减少推荐
            </button>
            <button class="text-button" @click="emit('comments', current.track)">评论</button>
            <button class="text-button" @click="emit('download', [current.track])">下载</button>
            <button class="text-button" :disabled="!transportActive" @click="player.toggleLyrics">
              歌词
            </button>
          </div>
          <button
            ref="returnButton"
            v-if="active"
            class="text-button fm-return"
            :disabled="pending"
            @click="leave"
          >
            返回原播放队列
          </button>
        </div>
      </div>
    </div>
    <div
      v-else
      class="fm-trash"
      :aria-busy="busy || trashLoading"
      :data-selected-count="selected.size"
    >
      <section v-if="view?.blockedArtists.length" class="fm-artists" aria-label="屏蔽的歌手">
        <h2>屏蔽的歌手</h2>
        <FmVirtualList :items="view.blockedArtists" :row-height="56" label="屏蔽的歌手">
          <template #default="{ item: artist }">
            <div :data-artist-id="artist.id" class="fm-trash-row">
              <strong :title="artist.name">{{ artist.name }}</strong>
              <button
                class="text-button"
                :disabled="writeLocked"
                @click="unblock(artist.id, $event)"
              >
                取消屏蔽
              </button>
            </div>
          </template>
        </FmVirtualList>
      </section>
      <div ref="trashToolbar" class="fm-trash-toolbar" tabindex="-1">
        <span>已加载 {{ trash?.songs.length ?? 0 }} 首 · 每次最多恢复 100 首</span
        ><button
          class="primary"
          :disabled="listLocked || restorePending || !selected.size"
          @click="restore"
        >
          恢复推荐{{ selected.size ? `（${selected.size}）` : '' }}
        </button>
      </div>
      <div v-if="model.trashIssue.value" class="fm-list-error error-state" role="alert">
        <span>{{ model.trashIssue.value.message }}</span>
        <button
          class="text-button"
          :disabled="trashLoading || busy"
          @click="readTrash($event, model.trashIssue.value.more)"
        >
          {{ model.trashIssue.value.more ? '重试加载更多' : '重新读取列表' }}
        </button>
      </div>
      <p v-if="restorePending" class="fm-notice" role="status">
        有恢复操作尚未确认，请先在反馈状态中核对。<button
          class="text-button"
          @click="recovery?.focus()"
        >
          查看反馈状态
        </button>
      </p>
      <div v-if="!trash" class="empty-state">{{ trashLoading ? '正在读取歌曲…' : '暂无列表' }}</div>
      <div v-else-if="!trash.songs.length" class="empty-state">这里还没有歌曲</div>
      <FmVirtualList
        v-if="trash?.songs.length"
        :items="trash.songs"
        :row-height="72"
        label="减少推荐的歌曲"
      >
        <template #default="{ item: track }">
          <div :data-song-id="track.id" class="fm-trash-row">
            <label
              ><input
                type="checkbox"
                :aria-label="`选择${track.name || `歌曲 ${track.id}`}`"
                :checked="selected.has(track.id)"
                :disabled="
                  listLocked || restorePending || (!selected.has(track.id) && selected.size >= 100)
                "
                @change="toggle(track.id)"
              /><span
                ><strong :title="track.name || `歌曲 ${track.id}`">{{
                  track.name || `歌曲 ${track.id}`
                }}</strong
                ><small>{{ artistLabel(track) }}</small></span
              ></label
            >
            <button
              class="text-button"
              :disabled="listLocked || restorePending"
              @click="restore($event, [track.id])"
            >
              恢复推荐
            </button>
          </div>
        </template>
      </FmVirtualList>
      <button
        v-if="trash?.more"
        class="text-button"
        :disabled="trashLoading || busy"
        @click="readTrash($event, true)"
      >
        {{ trashLoading ? '正在加载…' : '加载更多' }}
      </button>
    </div>
    <div
      ref="recovery"
      v-if="view?.operations.length"
      class="fm-recovery"
      tabindex="-1"
      aria-label="FM待处理操作"
    >
      <h3>反馈状态</h3>
      <div v-for="op in view.operations" :key="op.id">
        <template v-if="op.kind === 'artist'">
          <span>{{
            op.phase === 'rejected'
              ? op.desired
                ? '歌手屏蔽被拒绝'
                : '取消屏蔽被拒绝'
              : '歌手屏蔽状态尚未确认'
          }}</span>
          <button
            v-if="op.phase === 'rejected' && op.artistId"
            :disabled="writeLocked"
            @click="model.command({ kind: 'artist', id: op.artistId, blocked: op.desired ?? true })"
          >
            重试此操作
          </button>
          <button
            v-else
            :disabled="writeLocked"
            @click="model.command({ kind: 'reconcile_artist', id: op.id })"
          >
            核对状态
          </button>
        </template>
        <template v-else-if="op.kind === 'trash'"
          ><span>{{
            op.canApply
              ? op.artistId
                ? '歌曲已减少推荐，请核对歌手屏蔽并继续'
                : '减少推荐已确认，可以继续播放'
              : op.desired
                ? '减少推荐结果尚未确认'
                : '恢复推荐结果尚未确认'
          }}</span
          ><button :disabled="writeLocked" @click="reconcileFeedback(op, $event)">
            {{ op.desired ? '核对并继续' : '核对恢复状态' }}
          </button></template
        >
        <template v-else-if="op.phase === 'prepared'"
          ><span>有一条尚未发送的跳过反馈</span
          ><button
            :disabled="writeLocked"
            @click="model.command({ kind: 'dispatch_skip', id: op.id })"
          >
            发送反馈</button
          ><button
            :disabled="writeLocked"
            @click="model.command({ kind: 'abandon_skip', id: op.id })"
          >
            放弃
          </button></template
        >
        <span v-else>跳过反馈结果尚未确认，不会重复发送</span>
      </div>
    </div>
    <ModalTransition
      :restore-focus="feedback.accountCurrent.value && !afterFeedback"
      :return-focus="() => feedbackButton ?? null"
      :fallback-focus="() => heading ?? null"
      @left="feedbackLeft"
    >
      <FmFeedbackDialog
        v-if="feedback.open.value && feedback.target.value"
        :key="feedback.target.value.id"
        :name="feedback.target.value.name"
        :reasons="reasons"
        :loading="reasonsLoading"
        :busy="feedback.busy.value"
        :changed="feedback.changed.value"
        :review="feedback.review.value"
        :can-submit="feedback.canSubmit.value"
        :issue="feedback.issue.value?.message ?? ''"
        :reason-issue="model.reasonsIssue.value?.message ?? ''"
        :state-issue="model.stateIssue.value?.message ?? ''"
        :reading-state="!!model.stateIssue.value && loading"
        @close="feedback.close"
        @submit="feedback.submit"
        @reload="feedback.retryReasons"
        @recover="feedback.retryState"
        @blacklist="feedbackNavigate('trash')"
        @review="feedbackNavigate('recovery')"
      />
    </ModalTransition>
  </section>
</template>
<style scoped>
.fm-artists {
  margin-bottom: 24px;
}
.fm-page {
  padding: 28px 34px;
  min-width: 0;
}
.fm-header,
.fm-trash-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
}
.fm-state-region:focus-visible,
.fm-trash-toolbar:focus-visible,
.fm-recovery:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 3px;
  border-radius: 6px;
}
.fm-header h1 {
  font-size: 26px;
  margin: 0 0 8px;
}
.fm-header h1:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 4px;
  border-radius: 4px;
}
.fm-header p,
.fm-copy p {
  color: var(--text-secondary);
  margin: 0;
}
.fm-now {
  display: flex;
  gap: 42px;
  align-items: center;
  max-width: 920px;
  margin: 46px auto;
}
.fm-art {
  width: clamp(150px, 27vw, 310px);
  aspect-ratio: 1;
  flex-shrink: 0;
  border-radius: 16px;
  background: linear-gradient(145deg, var(--surface-hover), var(--surface-alt));
  box-shadow: 0 12px 36px #24223d15;
  display: grid;
  place-items: center;
  overflow: hidden;
}
.fm-art img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.fm-art :deep(svg) {
  width: 70px;
  height: 70px;
  color: var(--art-color);
}
.fm-copy {
  min-width: 0;
}
.fm-copy h2 {
  font-size: 27px;
  line-height: 1.4;
  margin: 14px 0 10px;
  overflow-wrap: anywhere;
}
.fm-label {
  font-size: 12px;
  color: var(--muted);
}
.fm-copy .fm-album {
  margin-top: 9px;
  font-size: 13px;
}
.fm-actions,
.fm-tools {
  display: flex;
  align-items: center;
  gap: 15px;
  flex-wrap: wrap;
  margin-top: 27px;
}
.fm-actions .primary,
.fm-tools button {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}
.fm-tools {
  gap: 12px;
  margin-top: 22px;
}
.fm-return {
  margin-top: 24px;
  color: var(--text-secondary);
}
.fm-notice,
.fm-recovery {
  padding: 14px 18px;
  background: var(--surface-alt);
  border-radius: 10px;
  margin: 16px 0;
}
.fm-notice button,
.fm-recovery button {
  margin-left: 12px;
}
.fm-recovery h3 {
  margin: 0 0 12px;
  font-size: 15px;
}
.fm-recovery > div {
  margin: 10px 0;
}
.fm-trash {
  margin-top: 26px;
}
.fm-trash-toolbar {
  margin-bottom: 12px;
}
.fm-trash-row {
  height: 100%;
  box-sizing: border-box;
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 12px 10px;
  border-bottom: 1px solid var(--line);
}
.fm-trash-row label {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 14px;
  min-width: 0;
}
.fm-trash-row strong,
.fm-trash-row small {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.fm-trash-row > strong {
  min-width: 0;
  flex: 1;
}
.fm-trash-row label span {
  min-width: 0;
}
.fm-trash-row small {
  margin-top: 4px;
  color: var(--text-secondary);
}
.fm-trash-row button {
  flex-shrink: 0;
}
.liked {
  color: var(--accent);
}
@media (max-width: 900px) {
  .fm-page {
    padding: 20px;
  }
  .fm-now {
    gap: 24px;
    margin: 30px 0;
  }
  .fm-copy h2 {
    font-size: 21px;
  }
  .fm-art {
    width: 170px;
  }
  .fm-tools {
    gap: 8px;
  }
  .fm-header p {
    font-size: 12px;
  }
}
@media (max-width: 620px) {
  .fm-now {
    flex-direction: column;
    align-items: flex-start;
  }
  .fm-art {
    width: 180px;
  }
  .fm-actions {
    margin-top: 18px;
  }
}
</style>
