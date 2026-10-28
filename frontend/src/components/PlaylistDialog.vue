<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import type { PlaylistManager } from '../composables/usePlaylists';
import type { AccountView, PlaylistView, TrackView } from '../services/contracts';
import { coverMessage, type CoverOutcome } from '../services/playlistCover';
import PlaylistCoverDialog from './PlaylistCoverDialog.vue';
import ModalTransition from './ModalTransition.vue';
import {
  playlistLength,
  playlistPending,
  type PlaylistMutationView,
  type PlaylistDialogTarget,
} from '../services/playlists';
import Icon from './Icon.vue';
import { useConfirmationFocus } from '../composables/useConfirmationFocus';
const props = defineProps<{
  manager: PlaylistManager;
  target: PlaylistDialogTarget;
  playlists: PlaylistView[];
  account: AccountView;
}>();
const emit = defineEmits<{
  close: [];
  open: [id: string];
  download: [track: TrackView];
  comments: [track: TrackView];
  coverChanged: [];
  expired: [];
  recoverCover: [id: string];
}>();
const { editor, busy, loading, error, result, operations, coverOperations, tags, tagError } =
  props.manager;
const dialog = ref<HTMLDialogElement>();
const nameInput = ref<HTMLInputElement>();
let focusedName = false;
const name = ref(''),
  description = ref(''),
  selectedTags = ref<string[]>([]),
  privateList = ref(true),
  destination = ref(''),
  link = ref('');
const confirmation = ref<'delete' | 'clear' | 'publish'>();
useConfirmationFocus(dialog, confirmation);
const localError = ref('');
const showTags = ref(false);
const coverOpen = ref(false);
let applyingCover = false;
const title = computed(() =>
  confirmation.value
    ? { delete: '删除歌单', clear: '清空歌曲', publish: '公开歌单' }[confirmation.value]
    : props.target.kind === 'create'
      ? '新建歌单'
      : props.target.kind === 'edit'
        ? '编辑歌单'
        : props.target.kind === 'history'
          ? '歌单操作记录'
          : props.target.kind === 'open'
            ? '打开歌单'
            : '歌曲操作',
);
const own = computed(() => props.playlists.filter((p) => p.owned && !p.favorite));
const categories = computed(() => [...new Set(tags.value.map((t) => t.category))]);
const pending = computed(() => result.value && playlistPending(result.value.phase));
const invalid = computed(
  () =>
    !name.value.trim() ||
    /[@#]/.test(name.value) ||
    playlistLength(name.value.trim()) > 40 ||
    playlistLength(description.value) > 1000,
);
let focus: HTMLElement | null = null;
onMounted(() => {
  focus = document.activeElement as HTMLElement;
  dialog.value?.showModal();
  props.manager.reset();
  destination.value = own.value[0]?.id ?? '';
  if (props.target.kind === 'create') void nextTick(() => nameInput.value?.focus());
  if (props.target.kind === 'edit') {
    const target = props.target;
    void props.manager.open(target.id).then(() => {
      if (target.cover && editor.value?.id === target.id) coverOpen.value = true;
    });
    void props.manager.loadTags();
  }
  if (props.target.kind === 'history') void props.manager.refreshOperations();
});
onUnmounted(() => {
  props.manager.reset();
  focus?.focus();
});
watch(editor, (p) => {
  if (p && props.target.kind === 'edit' && !applyingCover) {
    name.value = p.name;
    description.value = p.description;
    selectedTags.value = [...p.tags];
    if (!focusedName) {
      focusedName = true;
      void nextTick(() => nameInput.value?.focus());
    }
  }
});
async function coverSaved(outcome: CoverOutcome) {
  applyingCover = true;
  try {
    await props.manager.refreshAfterCover(
      props.account.epoch,
      outcome.task.playlistId,
      outcome.editor,
    );
    await nextTick();
  } finally {
    applyingCover = false;
  }
  emit('coverChanged');
}
function close() {
  if (!busy.value) emit('close');
}
function cancel() {
  if (busy.value) return;
  if (confirmation.value) confirmation.value = undefined;
  else close();
}
function toggleTag(tag: string) {
  if (selectedTags.value.includes(tag))
    selectedTags.value = selectedTags.value.filter((t) => t !== tag);
  else if (selectedTags.value.length < 3) selectedTags.value = [...selectedTags.value, tag];
}
function finish(value: PlaylistMutationView | undefined) {
  if (value && props.manager.saved(value.phase)) emit('close');
}
async function submit() {
  localError.value = '';
  if (props.target.kind === 'open') {
    let id = link.value.trim();
    if (!/^[1-9][0-9]*$/.test(id))
      try {
        const u = new URL(id);
        if (u.hostname !== 'music.163.com') throw new Error();
        id =
          new URLSearchParams((u.hash.includes('?') ? u.hash.split('?')[1] : u.search) || '').get(
            'id',
          ) ?? '';
      } catch {
        id = '';
      }
    if (!/^[1-9][0-9]*$/.test(id)) {
      localError.value = '请输入网易云音乐歌单分享链接';
      return;
    }
    emit('open', id);
    emit('close');
    return;
  }
  if (props.target.kind === 'create') {
    if (invalid.value) return;
    finish(
      await props.manager.mutate({ kind: 'create', name: name.value, private: privateList.value }),
    );
    return;
  }
  const p = editor.value;
  if (props.target.kind !== 'edit' || !p) return;
  if (confirmation.value) {
    if (confirmation.value === 'clear')
      finish(
        await props.manager.mutate({
          kind: 'tracks',
          id: p.id,
          revision: p.revision,
          change: 'clear',
          ids: [],
        }),
      );
    else
      finish(
        await props.manager.mutate({ kind: confirmation.value, id: p.id, revision: p.revision }),
      );
  } else if (!invalid.value)
    finish(
      await props.manager.mutate({
        kind: 'metadata',
        id: p.id,
        revision: p.revision,
        name: name.value,
        tags: selectedTags.value,
        description: description.value,
      }),
    );
}
async function trackAction(action: 'add' | 'delete' | 'up' | 'down') {
  if (props.target.kind !== 'track') return;
  const id = action === 'add' ? destination.value : props.target.playlist?.id;
  if (!id) return;
  finish(await props.manager.tracks(id, action, props.target.track.id));
}
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog"
    aria-labelledby="playlist-dialog-title"
    @cancel.prevent="cancel"
  >
    <header>
      <h2 id="playlist-dialog-title">{{ title }}</h2>
      <button
        class="icon-button"
        aria-label="关闭歌单操作"
        data-confirmation-fallback
        :disabled="busy"
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <p v-if="error || localError" class="error-state" role="alert">{{ localError || error }}</p>
    <div v-if="result" class="mutation-result" :data-phase="result.phase" role="status">
      {{ result.message
      }}<span v-if="result.phase === 'partial'"
        >（名称 {{ result.serviceCodes.name ?? '未确认' }} / 标签
        {{ result.serviceCodes.tags ?? '未确认' }} / 简介
        {{ result.serviceCodes.description ?? '未确认' }}）</span
      >
      <button
        v-if="playlistPending(result.phase)"
        class="text-button"
        :disabled="busy"
        @click="manager.reconcile(result.operationId)"
      >
        核对结果
      </button>
    </div>
    <div v-if="target.kind === 'history'" class="operation-list">
      <p class="muted">未确认的操作仅核对实际结果，避免重复提交。</p>
      <p v-if="!operations.length && !coverOperations.length" class="empty-state">
        没有待处理的歌单操作
      </p>
      <article v-for="op in coverOperations" :key="op.id">
        <strong>{{ playlists.find((p) => p.id === op.playlistId)?.name ?? '歌单' }} · 封面</strong>
        <p>{{ coverMessage(op) }}</p>
        <button class="text-button" @click="emit('recoverCover', op.playlistId)">处理封面</button>
      </article>
      <article v-for="op in operations" :key="op.operationId">
        <strong>{{ op.label || '歌单操作' }}</strong>
        <p>{{ op.message }}</p>
        <button
          v-if="playlistPending(op.phase)"
          class="text-button"
          :disabled="busy"
          @click="manager.reconcile(op.operationId)"
        >
          核对结果
        </button>
        <button
          v-else-if="op.playlistId"
          class="text-button"
          @click="
            emit('open', op.playlistId);
            emit('close');
          "
        >
          查看歌单
        </button>
      </article>
      <button class="text-button" :disabled="busy" @click="manager.refreshOperations">
        刷新记录
      </button>
    </div>
    <div v-else-if="target.kind === 'track'" class="track-menu">
      <button
        v-if="!target.track.local"
        class="text-button"
        @click="emit('comments', target.track)"
      >
        查看歌曲评论
      </button>
      <h3>{{ target.track.name }}</h3>
      <p class="muted">{{ target.track.artists.join(' / ') }}</p>
      <button
        class="download-button"
        :disabled="busy || target.track.missing || target.track.local"
        @click="emit('download', target.track)"
      >
        下载歌曲
      </button>
      <label
        >收藏到歌单<select
          v-model="destination"
          aria-label="选择目标歌单"
          :disabled="busy || !!pending"
        >
          <option v-for="p in own" :key="p.id" :value="p.id">{{ p.name }}</option>
        </select></label
      >
      <button
        class="primary"
        :disabled="!destination || busy || !!pending || target.track.local || target.track.missing"
        @click="trackAction('add')"
      >
        添加到歌单
      </button>
      <div v-if="target.playlist?.owned && !target.playlist.favorite" class="track-menu-actions">
        <p class="muted">当前歌单：{{ target.playlist.name }}</p>
        <button class="text-button" :disabled="busy || !!pending" @click="trackAction('up')">
          上移一首
        </button>
        <button class="text-button" :disabled="busy || !!pending" @click="trackAction('down')">
          下移一首
        </button>
        <button
          class="danger text-button"
          :disabled="busy || !!pending"
          @click="trackAction('delete')"
        >
          从当前歌单移除
        </button>
      </div>
    </div>
    <form v-else @submit.prevent="submit">
      <div v-if="target.kind === 'edit' && loading" class="empty-state">
        <span class="spinner" />正在读取歌单…
      </div>
      <button
        v-else-if="target.kind === 'edit' && !editor"
        type="button"
        class="text-button"
        @click="manager.open(target.id)"
      >
        重新读取歌单
      </button>
      <fieldset v-else :disabled="busy || !!pending">
        <template v-if="target.kind === 'open'"
          ><label
            >歌单分享链接<input
              v-model="link"
              aria-label="歌单分享链接"
              placeholder="粘贴网易云音乐歌单链接" /></label
        ></template>
        <template v-else-if="confirmation">
          <p class="confirm-copy">
            {{
              confirmation === 'delete'
                ? `删除「${editor?.name}」？`
                : confirmation === 'clear'
                  ? `清空「${editor?.name}」中的 ${editor?.trackIds.length} 首歌曲？`
                  : `将「${editor?.name}」设为公开歌单？`
            }}
          </p>
          <p class="muted">
            {{
              confirmation === 'delete'
                ? '删除后将从你的账号中移除该歌单。'
                : confirmation === 'clear'
                  ? '音乐文件和播放队列将保留。'
                  : '公开后，其他用户可以查看和收藏此歌单。'
            }}
          </p>
        </template>
        <template v-else>
          <div
            v-if="target.kind === 'edit' && editor?.owned && !editor.favorite"
            class="playlist-cover-edit"
          >
            <button type="button" aria-label="替换歌单封面" @click="coverOpen = true">
              <img v-if="editor.coverUrl" :src="editor.coverUrl" alt="歌单封面" />
              <Icon v-else name="music" />
              <span>替换图片</span>
            </button>
          </div>
          <label
            >歌单名称<input
              ref="nameInput"
              v-model="name"
              aria-label="歌单名称"
              aria-describedby="playlist-name-count"
              :aria-invalid="!name.trim() || /[@#]/.test(name) || playlistLength(name.trim()) > 40"
              placeholder="输入歌单标题"
              maxlength="80"
            /><small
              id="playlist-name-count"
              :class="{ 'form-warning': playlistLength(name.trim()) > 40 }"
              >{{ playlistLength(name.trim()) }} / 40 · 中文计 2 个字符</small
            ></label
          >
          <label v-if="target.kind === 'create'" class="private-checkbox"
            ><input
              v-model="privateList"
              type="checkbox"
              aria-label="设为私密歌单"
            />设为私密歌单</label
          >
          <p v-if="target.kind === 'create' && !privateList" class="muted">
            公开歌单可被其他用户查看和收藏。
          </p>
          <template v-if="target.kind === 'edit'">
            <label
              >标签<span class="tag-selection"
                ><button
                  v-for="tag in selectedTags"
                  :key="tag"
                  type="button"
                  @click="toggleTag(tag)"
                >
                  {{ tag }} ×</button
                ><button type="button" @click="showTags = !showTags">选择标签</button
                ><small>最多 3 个</small></span
              ></label
            >
            <div v-if="showTags" class="tag-picker">
              <p v-if="tagError">
                {{ tagError
                }}<button type="button" class="text-button" @click="manager.loadTags">
                  重新加载标签
                </button>
              </p>
              <p v-else-if="!tags.length">正在加载标签…</p>
              <section v-for="category in categories" :key="category">
                <strong>{{ category }}</strong>
                <div>
                  <button
                    v-for="tag in tags.filter((t) => t.category === category)"
                    :key="tag.name"
                    type="button"
                    :aria-pressed="selectedTags.includes(tag.name)"
                    :disabled="selectedTags.length >= 3 && !selectedTags.includes(tag.name)"
                    @click="toggleTag(tag.name)"
                  >
                    {{ tag.name }}
                  </button>
                </div>
              </section>
            </div>
            <label
              >简介<textarea
                v-model="description"
                aria-label="歌单简介"
                aria-describedby="playlist-description-count"
                :aria-invalid="playlistLength(description) > 1000"
                rows="5"
                maxlength="2000"
              /><small
                id="playlist-description-count"
                :class="{ 'form-warning': playlistLength(description) > 1000 }"
                >{{ playlistLength(description) }} / 1000</small
              ></label
            >
          </template>
          <p v-if="name && invalid" class="form-warning">
            名称不能为空、不能包含 @ 或 #，并请保持名称与简介在字数上限内。
          </p>
        </template>
        <footer>
          <button
            v-if="confirmation"
            type="button"
            class="text-button"
            data-confirmation-cancel
            @click="confirmation = undefined"
          >
            返回编辑
          </button>
          <button
            type="submit"
            class="primary"
            :disabled="!confirmation && target.kind !== 'open' && invalid"
          >
            {{
              busy
                ? '正在保存…'
                : confirmation === 'delete'
                  ? '确认删除'
                  : confirmation === 'clear'
                    ? '确认清空'
                    : confirmation === 'publish'
                      ? '确认公开'
                      : target.kind === 'create'
                        ? '创建歌单'
                        : target.kind === 'open'
                          ? '打开'
                          : '保存修改'
            }}
          </button>
        </footer>
        <div
          v-if="target.kind === 'edit' && editor?.owned && !editor.favorite && !confirmation"
          class="playlist-danger-actions"
        >
          <button
            type="button"
            class="text-button"
            data-confirmation-entry="clear"
            :disabled="!editor.trackIds.length || editor.hasLocal"
            @click="confirmation = 'clear'"
          >
            清空歌曲
          </button>
          <button
            v-if="editor.private"
            type="button"
            class="text-button"
            data-confirmation-entry="publish"
            @click="confirmation = 'publish'"
          >
            设为公开
          </button>
          <button
            type="button"
            class="text-button danger"
            data-confirmation-entry="delete"
            @click="confirmation = 'delete'"
          >
            删除歌单
          </button>
        </div>
      </fieldset>
    </form>
    <p v-if="busy" class="muted" role="status">正在提交并核对，请稍候…</p>
    <Teleport to="body"
      ><ModalTransition
        ><PlaylistCoverDialog
          v-if="coverOpen && editor"
          :account="account"
          :editor="editor"
          @close="
            coverOpen = false;
            manager.refreshOperations();
          "
          @saved="coverSaved"
          @expired="emit('expired')" /></ModalTransition
    ></Teleport>
  </dialog>
</template>
<style scoped>
.playlist-cover-edit {
  float: right;
  margin: 0 0 12px 20px;
}
.playlist-cover-edit + label {
  margin-top: 0;
}
.playlist-cover-edit button {
  position: relative;
  width: 112px;
  height: 112px;
  display: grid;
  place-items: center;
  background: var(--surface-alt);
  color: var(--muted);
  border-radius: 12px;
  overflow: hidden;
}
.playlist-cover-edit img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.playlist-cover-edit span {
  position: absolute;
  inset: auto 0 0;
  background: #0006;
  color: var(--on-accent);
  font-size: 11px;
  padding: 8px;
  text-align: center;
}
.playlist-cover-edit button:hover span {
  background: #0008;
}
.playlist-cover-edit .icon {
  width: 36px;
  height: 36px;
}
@media (max-height: 650px) {
  .playlist-cover-edit button {
    width: 88px;
    height: 88px;
  }
}
</style>
