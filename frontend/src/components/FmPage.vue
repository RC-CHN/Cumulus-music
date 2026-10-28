<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { FmModel } from '../composables/useFm';
import type { PlayerModel } from '../composables/usePlayer';
import type { TrackView } from '../services/contracts';
import Icon from './Icon.vue';
import FmExplore from './FmExplore.vue';
import type { FmCard } from '../services/fm';
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
const exploring = ref(false);
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
      quality: props.player.view.value?.requestedQuality ?? 'standard',
    });
  }
}
const current = computed(() => view.value?.items.find((i) => i.entryId === view.value?.current));
const pending = computed(() => busy.value || props.player.busy.value || !props.player.ready.value);
const playing = computed(() => view.value?.active && props.player.playing.value);
const dialog = ref<HTMLDialogElement>();
const target = ref<{ entryId: string; name: string }>();
watch(
  () => JSON.stringify(view.value?.source),
  () => {
    selected.value = new Set();
    dialog.value?.close();
  },
);
function toggle(id: string) {
  const next = new Set(selected.value);
  if (next.has(id)) next.delete(id);
  else if (next.size < 100) next.add(id);
  selected.value = next;
}
function openTrash() {
  tab.value = 'trash';
  selected.value = new Set();
  void props.model.loadTrash();
}
async function restore() {
  const ids = [...selected.value];
  await props.model.command({ kind: 'restore_trash', ids });
  selected.value = new Set();
  await props.model.loadTrash();
}
function askTrash() {
  if (!current.value) return;
  target.value = { entryId: current.value.entryId, name: current.value.track.name };
  dialog.value?.showModal();
  void props.model.loadReasons(target.value.entryId);
}
async function reduce(reasonKey?: string) {
  if (!target.value) return;
  await props.model.command(
    reasonKey
      ? { kind: 'feedback', entry_id: target.value.entryId, reason_key: reasonKey }
      : { kind: 'trash', entry_id: target.value.entryId },
  );
  dialog.value?.close();
}
function closeReasons() {
  target.value = undefined;
  props.model.clearReasons();
}
function play() {
  if (view.value?.active) {
    if (!current.value || view.value.pending || view.value.notice)
      void props.model.command({ kind: 'resume' });
    else
      void props.player.action(playing.value ? { kind: 'pause', paused: true } : { kind: 'play' });
  } else
    void props.model.command({
      kind: 'start',
      quality: props.player.view.value?.requestedQuality ?? 'standard',
    });
}
</script>
<template>
  <section class="fm-page" aria-label="私人FM" :data-fm-source="JSON.stringify(model.source.value)">
    <header class="fm-header">
      <div>
        <h1>{{ tab === 'fm' ? model.title.value : '音乐黑名单' }}</h1>
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
      <button class="text-button" @click="tab === 'fm' ? openTrash() : (tab = 'fm')">
        {{ tab === 'fm' ? '减少推荐列表' : '返回私人漫游' }}
      </button>
    </header>
    <FmExplore v-if="tab === 'fm' && exploring" :model="model" @select="selectCard" />
    <div v-if="error" class="error-state" role="alert">
      {{ error
      }}<button @click="tab === 'fm' ? model.load(true) : model.loadTrash()">重新加载</button>
    </div>
    <div v-if="view?.pending || view?.notice" class="fm-notice" role="status">
      {{ view.notice || '切歌尚未完成'
      }}<button :disabled="pending" @click="model.command({ kind: 'resume' })">重试继续播放</button>
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
          <p>{{ current?.track.artists.join(' / ') || '重新加载，发现更多音乐' }}</p>
          <p class="fm-album">{{ current?.track.album }}</p>
          <div class="fm-actions">
            <button class="primary" :disabled="pending" @click="play">
              <Icon :name="playing ? 'pause' : 'play'" />{{
                playing ? '暂停漫游' : view?.active ? '继续播放' : '开始漫游'
              }}
            </button>
            <button
              class="icon-button"
              aria-label="FM上一首"
              :disabled="pending || !view?.active || view.pending || !current"
              @click="player.action({ kind: 'previous' })"
            >
              <Icon name="previous" />
            </button>
            <button
              class="icon-button"
              aria-label="FM下一首"
              :disabled="pending || !view?.active || view.pending || !current"
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
              class="text-button"
              :disabled="pending || !view?.active || view.pending"
              @click="askTrash"
            >
              减少推荐
            </button>
            <button class="text-button" @click="emit('comments', current.track)">评论</button>
            <button class="text-button" @click="emit('download', [current.track])">下载</button>
            <button class="text-button" :disabled="!view?.active" @click="player.toggleLyrics">
              歌词
            </button>
          </div>
          <button
            v-if="view?.active"
            class="text-button fm-return"
            :disabled="pending"
            @click="model.command({ kind: 'leave' })"
          >
            返回原播放队列
          </button>
        </div>
      </div>
    </div>
    <div v-else class="fm-trash">
      <section v-if="view?.blockedArtists.length" class="fm-artists" aria-label="屏蔽的歌手">
        <h2>屏蔽的歌手</h2>
        <div
          v-for="artist in view.blockedArtists"
          :key="artist.id"
          :data-artist-id="artist.id"
          class="fm-trash-row"
        >
          <strong>{{ artist.name }}</strong>
          <button
            class="text-button"
            :disabled="busy"
            @click="model.command({ kind: 'artist', id: artist.id, blocked: false })"
          >
            取消屏蔽
          </button>
        </div>
      </section>
      <div class="fm-trash-toolbar">
        <span>已加载 {{ trash?.songs.length ?? 0 }} 首 · 每次最多恢复 100 首</span
        ><button
          class="primary"
          :disabled="busy || trashLoading || !selected.size"
          @click="restore"
        >
          恢复推荐{{ selected.size ? `（${selected.size}）` : '' }}
        </button>
      </div>
      <div v-if="!trash" class="empty-state">{{ trashLoading ? '正在读取歌曲…' : '暂无列表' }}</div>
      <div v-else-if="!trash.songs.length" class="empty-state">这里还没有歌曲</div>
      <div
        v-for="track in trash?.songs"
        :key="track.id"
        :data-song-id="track.id"
        class="fm-trash-row"
      >
        <label
          ><input
            type="checkbox"
            :aria-label="`选择${track.name}`"
            :checked="selected.has(track.id)"
            :disabled="busy || (!selected.has(track.id) && selected.size >= 100)"
            @change="toggle(track.id)"
          /><span
            ><strong>{{ track.name }}</strong
            ><small>{{ track.artists.join(' / ') }}</small></span
          ></label
        >
        <button
          class="text-button"
          :disabled="busy || trashLoading"
          @click="
            selected = new Set([track.id]);
            restore();
          "
        >
          恢复推荐
        </button>
      </div>
      <button
        v-if="trash?.more"
        class="text-button"
        :disabled="trashLoading || busy"
        @click="model.loadTrash(true)"
      >
        {{ trashLoading ? '正在加载…' : '加载更多' }}
      </button>
    </div>
    <div v-if="view?.operations.length" class="fm-recovery" aria-label="FM待处理操作">
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
            :disabled="busy"
            @click="model.command({ kind: 'artist', id: op.artistId, blocked: op.desired ?? true })"
          >
            重试此操作
          </button>
          <button
            v-else
            :disabled="busy"
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
          ><button :disabled="busy" @click="model.command({ kind: 'reconcile', id: op.id })">
            核对并继续
          </button></template
        >
        <template v-else-if="op.phase === 'prepared'"
          ><span>有一条尚未发送的跳过反馈</span
          ><button :disabled="busy" @click="model.command({ kind: 'dispatch_skip', id: op.id })">
            发送反馈</button
          ><button :disabled="busy" @click="model.command({ kind: 'abandon_skip', id: op.id })">
            放弃
          </button></template
        >
        <span v-else>跳过反馈结果尚未确认，不会重复发送</span>
      </div>
    </div>
    <dialog ref="dialog" class="fm-dialog" @close="closeReasons">
      <h2>减少推荐</h2>
      <p>减少推荐「{{ target?.name }}」，并播放下一首。之后可以在减少推荐列表中恢复。</p>
      <p v-if="target && target.entryId !== current?.entryId" role="status">
        当前歌曲已切换，请重新打开此窗口。
      </p>
      <p v-if="reasonsLoading">正在读取反馈选项…</p>
      <div v-if="reasons" class="fm-reasons">
        <button
          v-for="reason in reasons.options.filter((r) => r.artistId)"
          :key="reason.key"
          :data-reason-key="reason.key"
          :data-artist-id="reason.artistId"
          class="fm-reason"
          :disabled="busy || target?.entryId !== current?.entryId"
          @click="reduce(reason.key)"
        >
          {{ reason.label }}
        </button>
        <h3>歌曲反馈</h3>
        <button
          v-for="reason in reasons.options.filter((r) => !r.artistId)"
          :key="reason.key"
          class="fm-reason"
          :disabled="busy || target?.entryId !== current?.entryId"
          @click="reduce(reason.key)"
        >
          {{ reason.label }}
        </button>
      </div>
      <div v-if="error" class="error-state" role="alert">
        {{ error
        }}<button
          :disabled="busy || reasonsLoading || !target"
          @click="target && model.loadReasons(target.entryId)"
        >
          重新加载选项
        </button>
      </div>
      <div>
        <button :disabled="busy" @click="dialog?.close()">取消</button
        ><button
          class="primary"
          :disabled="busy || target?.entryId !== current?.entryId"
          @click="reduce()"
        >
          仅减少此歌曲
        </button>
      </div>
    </dialog>
  </section>
</template>
<style scoped>
.fm-dialog .fm-reasons {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin: 20px 0;
  justify-content: flex-start;
}
.fm-reasons h3 {
  width: 100%;
  margin: 10px 0 0;
}
.fm-reason {
  padding: 12px;
  border-radius: 8px;
  background: var(--surface-alt);
  max-width: 100%;
  white-space: normal;
}
.fm-reason:hover {
  background: var(--surface-alt);
  color: var(--primary);
}
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
.fm-header h1 {
  font-size: 26px;
  margin: 0 0 8px;
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
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 12px 10px;
  border-bottom: 1px solid var(--line);
}
.fm-trash-row label {
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
.fm-dialog {
  max-width: min(440px, 80vw);
  border: 0;
  border-radius: 14px;
  padding: 24px;
  box-shadow: 0 16px 70px #0003;
}
.fm-dialog::backdrop {
  background: #0005;
}
.fm-dialog p {
  line-height: 1.8;
}
.fm-dialog > div {
  display: flex;
  justify-content: flex-end;
  gap: 12px;
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
