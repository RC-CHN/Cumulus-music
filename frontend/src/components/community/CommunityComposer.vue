<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useCore } from '../../services/core';
import { failure } from '../../services/contracts';
import type {
  ComposerAttachment,
  ComposerAttachmentEdit,
  ComposerDraft,
  ComposerKind,
  ComposerPublication,
  ComposerSearchItem,
} from '../../services/community';
import AccountBindingQrDialog from '../account/AccountBindingQrDialog.vue';
import Icon from '../Icon.vue';
import ModalTransition from '../ModalTransition.vue';

const props = defineProps<{ epoch: string }>();
const emit = defineEmits<{ publishedDetail: [detail: string] }>();
const core = useCore();
const trigger = ref<HTMLButtonElement>();
const modal = ref<HTMLDialogElement>();
const body = ref<HTMLTextAreaElement>();
const searchInput = ref<HTMLInputElement>();
const opened = ref(false);
const pane = ref<'compose' | 'search'>('compose');
const saved = ref<ComposerDraft | null>();
const text = ref('');
const selected = ref<ComposerAttachment | null>(null);
const edit = ref<ComposerAttachmentEdit>({ kind: 'keep' });
const pending = ref(false);
const issue = ref('');
const notice = ref('');
const confirmClose = ref(false);
const confirmDelete = ref(false);
const confirmPublish = ref(false);
const cancelPublish = ref<HTMLButtonElement>();
const publishButton = ref<HTMLButtonElement>();
const publishedButton = ref<HTMLButtonElement>();
const recoveryButton = ref<HTMLButtonElement>();
const publicationMessage = ref<HTMLElement>();
const publication = ref<ComposerPublication>();
const publicationIssue = ref('');
const checking = ref(false);
const bindingQr = ref(false);
const uncertainLocalRevision = ref<string>();
const keyword = ref('');
const kind = ref<ComposerKind>('song');
const results = ref<ComposerSearchItem[]>([]);
const nextOffset = ref<number | null>(null);
const searched = ref(false);
const searchPending = ref(false);
const searchError = ref('');
let session = 0;
let searchGeneration = 0;
const kinds: { id: ComposerKind; label: string }[] = [
  { id: 'song', label: '单曲' },
  { id: 'artist', label: '歌手' },
  { id: 'album', label: '专辑' },
  { id: 'playlist', label: '歌单' },
  { id: 'podcast', label: '播客' },
];
const kindLabel = (value: ComposerKind) => kinds.find((kind) => kind.id === value)?.label;
const dirty = computed(
  () => text.value !== (saved.value?.text ?? '') || edit.value.kind !== 'keep',
);
const valid = computed(
  () => text.value.length <= 1000 && (!!text.value.trim() || !!selected.value),
);

const writeLocked = computed(
  () => publication.value?.status === 'uncertain' || !!uncertainLocalRevision.value,
);
const canPublish = computed(
  () =>
    !!publication.value &&
    !writeLocked.value &&
    !pending.value &&
    !checking.value &&
    valid.value &&
    !!text.value.trim() &&
    !!selected.value,
);
const publicationText = computed(() => {
  switch (publication.value?.status) {
    case 'published':
      return '已收到并保存发布成功回执。';
    case 'uncertain':
      return '无法确认是否已经发布。草稿已保留，客户端不会再次提交。';
    case 'phone_required':
      return '发布前需要绑定手机号。请在网易云音乐 App 完成绑定，再回来重新确认发布。';
    case 'preflight_unavailable':
      return '暂时无法核对发布资格，本次没有发出发布请求。';
    case 'rejected':
      return {
        cannot_share: '该资源无法分享，草稿已保留。',
        private_cloud: '该云盘资源暂不支持分享，草稿已保留。',
        purchase_required: '该资源需购买后才能分享，草稿已保留。',
        not_sent: '发布请求未发出，草稿已保留。',
      }[publication.value.reason];
    default:
      return '';
  }
});

function resetSearch() {
  searchGeneration++;
  results.value = [];
  nextOffset.value = null;
  searched.value = false;
  searchPending.value = false;
  searchError.value = '';
}
watch(keyword, resetSearch, { flush: 'sync' });

function resetDraft(value: ComposerDraft | null | undefined) {
  saved.value = value;
  text.value = value?.text ?? '';
  selected.value = value?.attachment ?? null;
  edit.value = { kind: 'keep' };
}
function end() {
  session++;
  resetSearch();
  resetDraft(undefined);
  pending.value = false;
  issue.value = '';
  notice.value = '';
  confirmClose.value = false;
  confirmDelete.value = false;
  confirmPublish.value = false;
  publication.value = undefined;
  publicationIssue.value = '';
  checking.value = false;
  bindingQr.value = false;
  pane.value = 'compose';
  opened.value = false;
}
watch(
  () => props.epoch,
  () => {
    uncertainLocalRevision.value = undefined;
    end();
  },
  { flush: 'sync' },
);
onBeforeUnmount(end);

function close() {
  if (pending.value) return;
  if (pane.value === 'search') return backToDraft();
  if (confirmPublish.value) return cancelPublication();
  if (dirty.value) {
    confirmClose.value = true;
    confirmDelete.value = false;
    confirmPublish.value = false;
  } else end();
}
async function open() {
  if (opened.value) return;
  opened.value = true;
  pane.value = 'compose';
  await nextTick();
  modal.value?.showModal();
  await read();
}
async function read() {
  if (!opened.value || pending.value) return;
  const epoch = props.epoch;
  const ticket = ++session;
  const live = () => opened.value && props.epoch === epoch && ticket === session;
  issue.value = '';
  pending.value = true;
  try {
    const [draft, status] = await Promise.allSettled([
      core.communityComposerDraft(epoch, { kind: 'read' }),
      core.communityComposerPublish(epoch, { kind: 'status' }),
    ]);
    if (!live()) return;
    const currentDraft =
      draft.status === 'fulfilled' && draft.value.epoch === epoch ? draft.value : undefined;
    if (currentDraft) resetDraft(currentDraft.value);
    // A separate host can finish publication between the two local reads.
    // Apply its receipt after hydration so the consumed draft cannot reappear.
    if (status.status === 'fulfilled' && status.value.epoch === epoch) {
      applyPublication(status.value.value);
      if (uncertainLocalRevision.value)
        publicationIssue.value = '本机还没有这次提交的回执，仍无法确认是否发布。';
    } else {
      publicationIssue.value = '暂时无法读取本机发布状态。';
    }
    if (!currentDraft) throw Error('could not read the current draft');
  } catch {
    if (live()) issue.value = '暂时无法读取草稿。';
  } finally {
    if (live()) {
      pending.value = false;
      await nextTick();
      body.value?.focus({ preventScroll: true });
    }
  }
}
async function store(discard = false) {
  if (!opened.value || saved.value === undefined || pending.value || writeLocked.value) return;
  const revision = saved.value?.revision ?? null;
  if (discard ? !revision : !valid.value || !dirty.value) return;
  const epoch = props.epoch;
  const ticket = ++session;
  const live = () => opened.value && props.epoch === epoch && ticket === session;
  issue.value = '';
  notice.value = '';
  pending.value = true;
  confirmDelete.value = false;
  try {
    const response = await core.communityComposerDraft(
      epoch,
      discard && revision
        ? { kind: 'discard', revision }
        : { kind: 'save', revision, text: text.value, attachment: edit.value },
    );
    if (live() && response.epoch === epoch) {
      resetDraft(response.value);
      notice.value = discard ? '已删除本机草稿。' : '草稿已保存在本机。';
    }
  } catch (error) {
    if (live())
      issue.value =
        failure(error).code === 'stale'
          ? '草稿或附件选择已失效。请重新打开草稿或重新选择附件。'
          : '暂时无法保存本机草稿。';
  } finally {
    if (live()) pending.value = false;
  }
}
function applyPublication(value: ComposerPublication) {
  if (
    !uncertainLocalRevision.value ||
    ('revision' in value && value.revision === uncertainLocalRevision.value)
  ) {
    uncertainLocalRevision.value = undefined;
    publication.value = value;
    publicationIssue.value = '';
    if (value.status === 'published' && saved.value?.revision === value.revision) resetDraft(null);
  }
}
async function checkPublication(mode: 'status' | 'check_own_notes' = 'status') {
  if (!opened.value || pending.value || checking.value) return;
  const epoch = props.epoch;
  const ticket = session;
  const live = () => opened.value && props.epoch === epoch && ticket === session;
  const origin = document.activeElement;
  checking.value = true;
  publicationIssue.value = '';
  try {
    const response = await core.communityComposerPublish(epoch, { kind: mode });
    if (live() && response.epoch === epoch) {
      applyPublication(response.value);
      if (uncertainLocalRevision.value)
        publicationIssue.value = '本机还没有这次提交的回执，仍无法确认是否发布。';
      else if (response.value.status === 'uncertain')
        notice.value =
          mode === 'status'
            ? '已核对本机记录，发布结果仍未确认。'
            : '最近笔记里没有找到可确认的回执，仍无法确认是否发布。';
      else if (mode === 'check_own_notes' && response.value.status === 'published') {
        notice.value = '';
        await nextTick();
        if (
          live() &&
          (document.activeElement === document.body || document.activeElement === origin)
        )
          publishedButton.value?.focus({ preventScroll: true });
      }
    }
  } catch {
    if (live())
      publicationIssue.value =
        mode === 'status' ? '暂时无法读取本机发布状态。' : '暂时无法读取最近笔记。';
  } finally {
    if (live()) checking.value = false;
  }
}
async function askPublication() {
  if (!canPublish.value || confirmPublish.value) return;
  confirmPublish.value = true;
  await nextTick();
  cancelPublish.value?.focus({ preventScroll: true });
}
async function cancelPublication() {
  confirmPublish.value = false;
  await nextTick();
  publishButton.value?.focus({ preventScroll: true });
}
async function publish() {
  if (!confirmPublish.value || !canPublish.value || saved.value === undefined) return;
  const epoch = props.epoch;
  const ticket = ++session;
  const live = () => opened.value && props.epoch === epoch && ticket === session;
  let submitted: string | undefined;
  pending.value = true;
  publicationIssue.value = '';
  issue.value = '';
  notice.value = '';
  try {
    if (dirty.value) {
      const stored = await core.communityComposerDraft(epoch, {
        kind: 'save',
        revision: saved.value?.revision ?? null,
        text: text.value,
        attachment: edit.value,
      });
      if (!live()) return;
      if (stored.epoch !== epoch || !stored.value) throw Error('draft not stored');
      resetDraft(stored.value);
    }
    submitted = saved.value?.revision;
    if (!submitted) throw Error('no current draft');
    const response = await core.communityComposerPublish(epoch, {
      kind: 'submit',
      revision: submitted,
    });
    if (!live()) return;
    if (response.epoch !== epoch) throw Error('stale publish response');
    applyPublication(response.value);
    if (response.value.status === 'phone_required') bindingQr.value = true;
  } catch {
    if (live()) {
      if (submitted) {
        uncertainLocalRevision.value = submitted;
        publicationIssue.value = '没有收到这次提交的结果，请先核对本机回执。';
      } else {
        publicationIssue.value = '本机草稿无法保存，未发出发布请求。';
      }
    }
  } finally {
    if (live()) {
      pending.value = false;
      confirmPublish.value = false;
      await nextTick();
      if (!live() || bindingQr.value) return;
      (
        publishedButton.value ??
        recoveryButton.value ??
        publicationMessage.value ??
        publishButton.value
      )?.focus({ preventScroll: true });
    }
  }
}
async function closeBindingQr() {
  bindingQr.value = false;
  await nextTick();
  if (opened.value) publishButton.value?.focus({ preventScroll: true });
}
function showPublished() {
  if (publication.value?.status !== 'published') return;
  const target = publication.value.detail;
  end();
  emit('publishedDetail', target);
}
async function openSearch() {
  pane.value = 'search';
  keyword.value = '';
  kind.value = 'song';
  resetSearch();
  await nextTick();
  searchInput.value?.focus({ preventScroll: true });
}
async function backToDraft() {
  resetSearch();
  pane.value = 'compose';
  await nextTick();
  body.value?.focus({ preventScroll: true });
}
async function switchKind(value: ComposerKind) {
  if (kind.value === value) return;
  kind.value = value;
  resetSearch();
  if (keyword.value.trim()) await search();
}
async function search(offset = 0) {
  const term = keyword.value.trim();
  if (!term || searchPending.value || pane.value !== 'search') return;
  const epoch = props.epoch;
  const type = kind.value;
  const ticket = ++searchGeneration;
  const live = () =>
    opened.value && pane.value === 'search' && epoch === props.epoch && ticket === searchGeneration;
  searchPending.value = true;
  searchError.value = '';
  if (!offset) {
    results.value = [];
    nextOffset.value = null;
    searched.value = false;
  }
  try {
    const response = await core.communityComposerSearch(epoch, type, term, offset);
    if (live() && response.epoch === epoch) {
      results.value = [...results.value, ...response.value.items];
      nextOffset.value = response.value.nextOffset;
      searched.value = true;
    }
  } catch {
    if (live()) searchError.value = '暂时无法读取附件搜索结果。';
  } finally {
    if (live()) searchPending.value = false;
  }
}
function choose(item: ComposerSearchItem) {
  selected.value = item;
  edit.value = { kind: 'select', receipt: item.receipt };
  notice.value = '';
  void backToDraft();
}
function removeAttachment() {
  selected.value = null;
  edit.value = { kind: saved.value?.attachment ? 'remove' : 'keep' };
  notice.value = '';
}
</script>

<template>
  <button
    ref="trigger"
    type="button"
    class="primary"
    aria-haspopup="dialog"
    data-community-opener="composer"
    @click="open"
  >
    写笔记
  </button>
  <ModalTransition :return-focus="() => trigger ?? null">
    <dialog
      v-if="opened"
      ref="modal"
      class="playlist-dialog community-composer"
      aria-labelledby="community-composer-title"
      @cancel.prevent="close"
    >
      <header>
        <h2 id="community-composer-title">{{ pane === 'compose' ? '写笔记' : '选择音乐附件' }}</h2>
        <button type="button" class="icon-button" aria-label="关闭笔记" @click="close">
          <Icon name="close" />
        </button>
      </header>
      <template v-if="pane === 'compose'">
        <p class="muted">草稿保存在本机。公开发布需要正文和一项音乐附件。</p>
        <p
          v-if="publicationText"
          ref="publicationMessage"
          role="status"
          tabindex="-1"
          data-community-publication
        >
          {{ publicationText }}
        </p>
        <p v-if="publicationIssue" role="alert" data-community-publication-error>
          {{ publicationIssue }}
        </p>
        <div
          v-if="publication || publicationIssue || writeLocked"
          class="community-publication-actions"
        >
          <button
            v-if="publication?.status === 'phone_required'"
            class="text-button"
            type="button"
            :disabled="pending"
            @click="bindingQr = true"
          >
            打开绑定手机二维码
          </button>
          <button
            v-if="publication?.status === 'published'"
            ref="publishedButton"
            class="text-button"
            type="button"
            :disabled="pending"
            @click="showPublished"
          >
            查看已发布笔记
          </button>
          <button
            v-if="writeLocked || !publication"
            ref="recoveryButton"
            class="text-button"
            type="button"
            :disabled="pending || checking"
            @click="checkPublication()"
          >
            核对本机回执
          </button>
          <button
            v-if="publication?.status === 'uncertain'"
            class="text-button"
            type="button"
            :disabled="pending || checking"
            @click="checkPublication('check_own_notes')"
          >
            核对最近笔记
          </button>
        </div>
        <p v-if="saved === undefined && pending" role="status">正在读取本机草稿…</p>
        <p v-if="issue" role="alert">
          {{ issue }}
          <button v-if="saved === undefined" type="button" class="text-button" @click="read">
            重试
          </button>
        </p>
        <p v-if="notice" role="status">{{ notice }}</p>
        <template v-if="saved !== undefined">
          <label class="community-note-body">
            笔记正文
            <textarea
              ref="body"
              v-model="text"
              rows="4"
              maxlength="1000"
              :disabled="pending || writeLocked || confirmPublish"
              placeholder="一起聊聊音乐吧~"
              @input="notice = ''"
            />
          </label>
          <p class="muted community-note-count">{{ text.length }} / 1000</p>
          <div v-if="selected" class="community-note-attachment">
            <span>{{ kindLabel(selected.kind) }}：{{ selected.title }}</span>
            <button
              type="button"
              class="text-button"
              :disabled="pending || writeLocked || confirmPublish"
              @click="removeAttachment"
            >
              移除附件
            </button>
          </div>
          <button
            type="button"
            class="secondary"
            :disabled="pending || writeLocked || confirmPublish"
            @click="openSearch"
          >
            {{ selected ? '更换音乐附件' : '添加音乐附件' }}
          </button>
          <p v-if="confirmPublish" role="alert" data-community-publication-confirm>
            确定公开发布这条笔记及所选附件？
          </p>
          <p v-else-if="confirmClose" role="alert">这次修改尚未保存。</p>
          <p v-else-if="confirmDelete" role="alert">删除已保存在本机的草稿？</p>
          <footer>
            <template v-if="confirmPublish">
              <button
                ref="cancelPublish"
                type="button"
                class="secondary"
                :disabled="pending"
                @click="cancelPublication"
              >
                取消
              </button>
              <button type="button" class="primary" :disabled="pending" @click="publish">
                {{ pending ? '正在提交…' : '确认公开发布' }}
              </button>
            </template>
            <template v-else-if="confirmClose">
              <button type="button" class="secondary" @click="confirmClose = false">
                继续编辑
              </button>
              <button type="button" class="primary" @click="end">放弃未保存的修改</button>
            </template>
            <template v-else-if="confirmDelete">
              <button type="button" class="secondary" @click="confirmDelete = false">取消</button>
              <button type="button" class="primary" @click="store(true)">确认删除草稿</button>
            </template>
            <template v-else>
              <button
                v-if="saved"
                type="button"
                class="text-button"
                :disabled="pending || writeLocked"
                @click="confirmDelete = true"
              >
                删除本机草稿
              </button>
              <button
                type="button"
                class="primary"
                :disabled="pending || writeLocked || !valid || !dirty"
                @click="store()"
              >
                {{ pending ? '正在保存…' : '保存草稿' }}
              </button>
              <button
                ref="publishButton"
                type="button"
                class="primary"
                :disabled="!canPublish"
                @click="askPublication"
              >
                发布笔记
              </button>
            </template>
          </footer>
        </template>
      </template>
      <template v-else>
        <form class="community-note-search" @submit.prevent="search()">
          <input
            ref="searchInput"
            v-model="keyword"
            type="search"
            aria-label="搜索音乐附件"
            maxlength="200"
            placeholder="搜索音乐附件"
          />
          <button type="submit" class="primary" :disabled="!keyword.trim() || searchPending">
            搜索
          </button>
        </form>
        <nav class="community-note-tabs" aria-label="附件类别">
          <button
            v-for="type in kinds"
            :key="type.id"
            type="button"
            class="text-button"
            :aria-pressed="type.id === kind"
            @click="switchKind(type.id)"
          >
            {{ type.label }}
          </button>
        </nav>
        <p v-if="searchPending" role="status">正在搜索…</p>
        <p v-if="searchError" role="alert">
          {{ searchError }}
          <button type="button" class="text-button" @click="search(nextOffset ?? 0)">重试</button>
        </p>
        <p v-else-if="searched && !results.length && !searchPending">没有可选的音乐附件。</p>
        <ul v-if="results.length" class="community-note-results">
          <li v-for="item in results" :key="item.receipt">
            <button type="button" @click="choose(item)">
              <img v-if="item.coverUrl" :src="item.coverUrl" alt="" referrerpolicy="no-referrer" />
              <Icon v-else name="music" />
              <span>
                <strong>{{ item.title }}</strong>
                <small v-if="item.subtitle">{{ item.subtitle }}</small>
              </span>
            </button>
          </li>
        </ul>
        <button
          v-if="nextOffset !== null && !searchError"
          type="button"
          class="text-button"
          :disabled="searchPending"
          @click="search(nextOffset)"
        >
          加载更多附件
        </button>
        <footer>
          <button type="button" class="secondary" @click="backToDraft">返回笔记</button>
        </footer>
      </template>
    </dialog>
  </ModalTransition>
  <AccountBindingQrDialog v-if="bindingQr" :epoch="epoch" @close="closeBindingQr" />
</template>

<style scoped>
.community-composer {
  width: min(520px, calc(100vw - 32px));
  max-height: calc(100dvh - 32px);
}
.community-note-body textarea {
  width: 100%;
  resize: vertical;
}
.community-note-count {
  text-align: right;
  margin: 0 0 10px;
}
.community-note-attachment,
.community-publication-actions,
.community-note-search,
.community-note-tabs {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
}
.community-note-attachment {
  justify-content: space-between;
  overflow-wrap: anywhere;
}
.community-note-search input {
  flex: 1;
  min-width: 0;
}
.community-note-tabs {
  flex-wrap: wrap;
}
.community-note-tabs [aria-pressed='true'] {
  color: var(--accent);
  font-weight: 650;
}
.community-note-results {
  max-height: 256px;
  overflow: auto;
  padding: 0;
  list-style: none;
}
.community-note-results button {
  display: flex;
  align-items: center;
  text-align: left;
  gap: 10px;
  width: 100%;
  min-height: 64px;
  padding: 8px;
}
.community-note-results img,
.community-note-results button > .icon {
  width: 48px;
  height: 48px;
  flex-shrink: 0;
  border-radius: 6px;
  object-fit: cover;
}
.community-note-results small {
  display: block;
}
</style>
