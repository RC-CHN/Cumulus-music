<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  reactive,
  ref,
  shallowRef,
  watch,
} from 'vue';
import { useDownloadCollectCreation } from '../composables/useDownloadCollectCreation';
import type { PlaylistManager } from '../composables/usePlaylists';
import type { DownloadCollectScope } from '../services/downloads';
import { failure, type PlaylistView, type TrackView } from '../services/contracts';
import { useCore } from '../services/core';
import {
  playlistLength,
  playlistPending,
  type PlaylistMixedBatchSource,
  type PlaylistMutationView,
} from '../services/playlists';
import BoundedPanelTransition from './BoundedPanelTransition.vue';
import Icon from './Icon.vue';
import PlaylistCollectList from './PlaylistCollectList.vue';
import '../styles/playlist-collect.css';

const props = defineProps<{
  manager: PlaylistManager;
  epoch: string;
  tracks: TrackView[];
  ordinaryCatalog?: boolean;
  mixedBatchSource?: PlaylistMixedBatchSource;
  playlists: PlaylistView[];
  selectionSummary?: { total: number; unsupported: number; duplicates: number };
  downloadCollection?: DownloadCollectScope;
}>();
const emit = defineEmits<{
  close: [];
  expired: [];
  collected: [result: PlaylistMutationView];
  downloadCreationChanged: [];
  downloadCreationComplete: [];
}>();
const client = useCore();
const dialog = ref<HTMLDialogElement>();
const nameInput = ref<HTMLInputElement>();
const closeButton = ref<HTMLButtonElement>();
const creationDecision = ref<HTMLButtonElement>();
const { busy: managerBusy, error, result } = props.manager;
let alive = true;
const downloadCreation = useDownloadCollectCreation(
  client,
  () =>
    alive && props.downloadCollection?.epoch === props.epoch ? props.downloadCollection : undefined,
  () => emit('downloadCreationChanged'),
  () => void props.manager.refreshOperations(),
  () => emit('expired'),
);
const { phase: creationPhase, notice: creationNotice, locked: creationLocked } = downloadCreation;
const busy = computed(() => managerBusy.value || downloadCreation.busy.value);
watch(creationPhase, async (phase) => {
  if (!props.downloadCollection || phase === 'prepared' || phase === 'complete') return;
  await nextTick();
  if (alive && creationPhase.value === phase && !busy.value)
    (creationDecision.value ?? closeButton.value)?.focus({ preventScroll: true });
});

const interrupted = shallowRef<PlaylistMutationView>();
const likedDestination = ref('');
const receipt = computed(() => result.value ?? interrupted.value);
type Sort = 'default' | 'recently';
const sortOptions = [
  { value: 'default', label: '默认排序' },
  { value: 'recently', label: '常用优先' },
] as const;
const preference = 'cumulus.playlist-collect-sort.v1';
function rememberedSort(): Sort {
  try {
    return localStorage.getItem(preference) === 'recently' ? 'recently' : 'default';
  } catch {
    return 'default';
  }
}
const sort = ref<Sort>(rememberedSort());
function initial(rows: PlaylistView[]) {
  return { rows, next: 0 as number | null, loading: false, error: '', top: 0 };
}
const bySort = reactive({
  default: initial(
    props.downloadCollection
      ? []
      : props.playlists.filter((playlist) => playlist.owned && !playlist.favorite),
  ),
  recently: initial([]),
});
const current = computed(() => bySort[sort.value]);
const creating = ref(false),
  name = ref(''),
  privateList = ref(true);
const selected = ref(''),
  created = ref('');
const pending = computed(() => !!receipt.value && playlistPending(receipt.value.phase));
const fixedDestination = computed(() => !!props.downloadCollection && !!receipt.value);
const invalid = computed(
  () => !name.value.trim() || /[@#]/.test(name.value) || playlistLength(name.value.trim()) > 40,
);
onBeforeUnmount(() => {
  alive = false;
});
onMounted(() => {
  props.manager.reset();
  dialog.value?.showModal();
  void reload();
  void loadLikedDestination();
});
async function loadLikedDestination() {
  const scope = props.downloadCollection;
  if (!scope || scope.ownedOnly) return;
  try {
    const response = await client.downloadCollectLikedDestination(scope.epoch, scope.receipt);
    if (alive && props.downloadCollection === scope && response.epoch === props.epoch)
      likedDestination.value = response.value?.receipt ?? '';
  } catch (error) {
    if (!alive || props.downloadCollection !== scope) return;
    if (['session_expired', 'profile_changed'].includes(failure(error).code)) emit('expired');
  }
}
async function reload(automatic = false) {
  const order = sort.value;
  const state = bySort[order];
  const page = state.next;
  if (
    busy.value ||
    pending.value ||
    fixedDestination.value ||
    state.loading ||
    page === null ||
    (automatic && state.error)
  )
    return;
  const focused = dialog.value?.querySelector('.collect-load button:focus, .collect-more:focus');
  state.loading = true;
  try {
    const source = props.downloadCollection;
    const response = source
      ? await client.downloadCollectDestinations(source.epoch, source.receipt, order, page)
      : await client.playlistCollectPage(props.epoch, order, page);
    if (!alive || response.epoch !== props.epoch) return;
    const rows = page === 0 ? [] : state.rows;
    const known = new Set(rows.map((playlist) => playlist.id));
    state.rows = [
      ...rows,
      ...response.value.playlists.filter((playlist) => !known.has(playlist.id)),
    ];
    state.next = response.value.nextPage;
    state.error = '';
  } catch (e) {
    if (!alive) return;
    const issue = failure(e);
    state.error = issue.message;
    if (['session_expired', 'profile_changed'].includes(issue.code)) emit('expired');
  } finally {
    if (alive) {
      state.loading = false;
      if (focused && order === sort.value) {
        await nextTick();
        if (alive && order === sort.value && document.activeElement === document.body)
          dialog.value?.querySelector<HTMLElement>('.collect-list')?.focus({ preventScroll: true });
      }
    }
  }
}
function selectSort(order: Sort) {
  if (order === sort.value || busy.value || pending.value || fixedDestination.value) return;
  sort.value = order;
  try {
    localStorage.setItem(preference, order);
  } catch {
    // A session still keeps its active sort when local preferences are unavailable.
  }
  if (current.value.next === 0 && !current.value.error) void reload();
}
function sortKey(event: KeyboardEvent) {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
  if (busy.value || pending.value || fixedDestination.value) return;
  event.preventDefault();
  const next =
    event.key === 'Home'
      ? 'default'
      : event.key === 'End'
        ? 'recently'
        : sort.value === 'default'
          ? 'recently'
          : 'default';
  selectSort(next);
  void nextTick(() =>
    dialog.value?.querySelector<HTMLButtonElement>(`[data-collect-sort="${next}"]`)?.focus(),
  );
}
function close() {
  if (!busy.value) emit('close');
}
function startCreate() {
  if (
    busy.value ||
    pending.value ||
    fixedDestination.value ||
    (props.downloadCollection && (!props.downloadCollection.canCreate || creationLocked.value))
  )
    return;
  props.manager.reset();
  created.value = '';
  if (props.downloadCollection) {
    name.value = '';
    privateList.value = props.downloadCollection.origin === 'history';
  }
  creating.value = true;
}
function back() {
  if (busy.value || pending.value) return;
  props.manager.reset();
  creating.value = false;
}
function entered() {
  if (creating.value) nameInput.value?.focus();
  else dialog.value?.querySelector<HTMLElement>('.collect-list')?.focus({ preventScroll: true });
}
async function perform(
  operation: () => Promise<PlaylistMutationView | undefined>,
  kind: 'create' | 'tracks',
  id?: string,
) {
  const before = new Set(props.manager.operations.value.map((entry) => entry.operationId));
  interrupted.value = undefined;
  const value = await operation();
  if (!alive) return;
  if (!value && !props.downloadCollection) {
    // The core can persist an operation and then fail its authoritative readback.
    // Recover only this attempt's new journal entry, never a previous creation.
    interrupted.value = props.manager.operations.value.find(
      (entry) =>
        !before.has(entry.operationId) &&
        entry.kind === kind &&
        playlistPending(entry.phase) &&
        (kind === 'create' ? entry.label === name.value.trim() : entry.playlistId === id),
    );
  }
  return value;
}
async function collected(value: PlaylistMutationView | undefined) {
  if (!alive || !value || !props.manager.saved(value.phase)) return;
  if (value.mixedTrackAddition === 'earlier') return;
  if (value.kind === 'create' && value.playlistId) {
    // A confirmed creation is retained if adding the songs later fails. Never
    // create a second playlist when retrying the second half of this flow.
    created.value = value.playlistId;
    await collect(value.playlistId);
  } else {
    emit('collected', value);
    emit('close');
  }
}
async function collect(id: string) {
  if (!alive || busy.value || pending.value || fixedDestination.value) return;
  const scope = props.downloadCollection;
  selected.value = id;
  await collected(
    await perform(
      () =>
        scope
          ? props.manager.addDownloadedTrack(scope, id)
          : props.manager.addTracks(
              id,
              props.tracks.map((track) => track.id),
              props.ordinaryCatalog === true,
              () => alive,
              props.mixedBatchSource,
            ),
      'tracks',
      id,
    ),
  );
}
async function collectLiked() {
  const scope = props.downloadCollection;
  const destination = likedDestination.value;
  if (!scope || !destination || busy.value || pending.value || fixedDestination.value) return;
  selected.value = 'system-liked';
  await collected(await props.manager.collectDownloadedLiked(scope, destination, false));
}
async function submit() {
  if (busy.value || pending.value || (!created.value && invalid.value)) return;
  if (props.downloadCollection) {
    await downloadCreation.create(name.value.trim(), privateList.value);
    finishDownloadCreation();
    return;
  }
  if (created.value) await collect(created.value);
  else
    await collected(
      await perform(
        () =>
          props.manager.mutate({
            kind: 'create',
            name: name.value.trim(),
            private: privateList.value,
          }),
        'create',
      ),
    );
}
function finishDownloadCreation() {
  if (alive && creationPhase.value === 'complete') {
    emit('downloadCreationComplete');
    emit('close');
  }
}
async function resolveDownloadCreation(continueAdd: boolean) {
  if (busy.value) return;
  if (continueAdd) await downloadCreation.continueAdd();
  else await downloadCreation.reconcile();
  finishDownloadCreation();
}
async function reconcile() {
  if (!receipt.value || busy.value) return;
  const scope = props.downloadCollection;
  const current = receipt.value;
  if (current.mixedTrackAddition && current.playlistId) {
    await collected(
      await props.manager.reconcileMixedAdd(
        current.playlistId,
        current.operationId,
        current.mixedTrackAddition,
      ),
    );
  } else if (scope && current.verifiedLikedTrackIds && likedDestination.value) {
    await collected(
      await props.manager.collectDownloadedLiked(scope, likedDestination.value, true),
    );
  } else await collected(await props.manager.reconcile(current.operationId));
}
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog playlist-collect-dialog"
    aria-labelledby="collect-title"
    :aria-busy="busy"
    :data-download-collect="downloadCollection && !downloadCollection.origin ? '' : undefined"
    :data-history-collect="downloadCollection?.origin === 'history' ? '' : undefined"
    :data-lyric-group-collect="downloadCollection?.origin === 'search_lyrics' ? '' : undefined"
    @cancel.prevent="close"
  >
    <header>
      <button
        v-if="creating && !downloadCollection"
        class="icon-button collect-back"
        aria-label="返回收藏歌单列表"
        :disabled="busy || pending"
        @click="back"
      >
        <Icon name="back" />
      </button>
      <h2 id="collect-title">
        {{ creating ? (downloadCollection ? '创建歌单' : '创建新歌单') : '收藏到歌单' }}
      </h2>
      <button
        ref="closeButton"
        class="icon-button collect-close"
        aria-label="关闭歌单操作"
        :disabled="busy"
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <p class="sr-only">
      {{ tracks.length === 1 ? `收藏${tracks[0]?.name}` : `收藏${tracks.length}首歌曲` }}
    </p>
    <p
      v-if="downloadCollection?.origin === 'history' && selectionSummary"
      class="collect-selection-summary"
      role="status"
    >
      所选播放记录共 {{ selectionSummary.total }} 项，将收藏 {{ tracks.length }} 首歌曲。<template
        v-if="selectionSummary.unsupported"
        >{{ selectionSummary.unsupported }} 项无法收藏，已略过。</template
      >
    </p>
    <p
      v-else-if="downloadCollection?.origin === 'search_lyrics' && selectionSummary"
      class="collect-selection-summary"
      role="status"
    >
      所选歌词结果共 {{ selectionSummary.total }} 首，将收藏 {{ tracks.length }} 首。<template
        v-if="selectionSummary.unsupported"
        >{{ selectionSummary.unsupported }} 首无法收藏，已略过。</template
      >
    </p>
    <p
      v-else-if="downloadCollection?.ownedOnly && selectionSummary"
      class="collect-selection-summary"
      role="status"
    >
      已选 {{ selectionSummary.total }} 项下载，将收藏 {{ tracks.length }} 首歌曲。<template
        v-if="selectionSummary.unsupported"
        >{{ selectionSummary.unsupported }} 项歌曲无法收藏，已略过。</template
      >
    </p>
    <p v-else-if="selectionSummary" class="collect-selection-summary" role="status">
      播放列表共 {{ selectionSummary.total }} 项，将收藏 {{ tracks.length }} 首歌曲。<template
        v-if="selectionSummary.unsupported"
        >{{ selectionSummary.unsupported }} 项声音或未匹配音乐无法收藏到歌单。</template
      ><template v-if="selectionSummary.duplicates"
        >{{ selectionSummary.duplicates }} 项重复歌曲已合并。</template
      >
    </p>
    <div v-if="error && !pending" class="collect-feedback error-state" role="alert">
      {{ error }}
    </div>
    <div
      v-if="receipt && (!manager.saved(receipt.phase) || receipt.mixedTrackAddition === 'earlier')"
      class="collect-feedback mutation-result"
      :data-phase="receipt.phase"
      role="status"
    >
      {{ receipt.message }}
      <template v-if="downloadCollection && !pending">请关闭后重新选择目标歌单。</template>
      <button v-if="pending" class="text-button" :aria-disabled="busy" @click="reconcile">
        核对结果
      </button>
    </div>
    <div class="collect-step-viewport">
      <BoundedPanelTransition name="collect-step" @after-enter="entered">
        <form
          v-if="creating"
          key="create"
          class="collect-create"
          :data-download-create-phase="downloadCollection ? creationPhase : undefined"
          @submit.prevent="submit"
        >
          <fieldset
            :disabled="busy || pending || !!created || (downloadCollection && creationLocked)"
          >
            <label
              >歌单名称<input
                ref="nameInput"
                v-model="name"
                aria-label="歌单名称"
                aria-describedby="collect-name-count"
                :aria-invalid="!!name && invalid"
                placeholder="输入歌单标题"
                maxlength="80"
              />
              <small
                id="collect-name-count"
                :class="{ 'form-warning': playlistLength(name.trim()) > 40 }"
              >
                {{ playlistLength(name.trim()) }} / 40 · 中文计 2 个字符
              </small></label
            >
            <label class="private-checkbox"
              ><input
                v-model="privateList"
                type="checkbox"
                :aria-label="downloadCollection ? '设为隐私歌单' : '设为私密歌单'"
              />{{ downloadCollection ? '设为隐私歌单' : '设为私密歌单' }}</label
            >
            <p v-if="!privateList" class="muted">公开歌单可被其他用户查看和收藏。</p>
            <p v-if="name && invalid" class="form-warning">
              名称不能包含 @ 或 #，并请保持在字数上限内。
            </p>
          </fieldset>
          <p v-if="created" class="muted" role="status">歌单已创建，继续将歌曲收藏到这个歌单。</p>
          <p v-if="downloadCollection && creationNotice" role="status" data-download-create-notice>
            {{ creationNotice }}
          </p>
          <footer>
            <button
              v-if="downloadCollection && creationPhase === 'pending'"
              ref="creationDecision"
              type="button"
              class="secondary"
              :disabled="busy"
              @click="resolveDownloadCreation(false)"
            >
              核对结果
            </button>
            <button
              v-else-if="downloadCollection && creationPhase === 'created_ready'"
              ref="creationDecision"
              type="button"
              class="primary"
              :disabled="busy"
              @click="resolveDownloadCreation(true)"
            >
              继续收藏
            </button>
            <button
              v-else-if="!downloadCollection || creationPhase === 'prepared'"
              type="submit"
              class="primary"
              :disabled="!created && invalid"
              :aria-disabled="busy || pending"
            >
              {{
                busy
                  ? '正在收藏…'
                  : created
                    ? '继续收藏'
                    : downloadCollection
                      ? '创建'
                      : '创建并收藏'
              }}
            </button>
          </footer>
        </form>
        <div v-else key="list" class="collect-body">
          <div
            class="collect-sort-tabs"
            role="tablist"
            aria-label="收藏歌单排序"
            @keydown="sortKey"
          >
            <button
              v-for="option in sortOptions"
              :key="option.value"
              role="tab"
              type="button"
              :data-collect-sort="option.value"
              :aria-selected="sort === option.value"
              :tabindex="sort === option.value ? 0 : -1"
              :aria-disabled="busy || pending || fixedDestination"
              @click="selectSort(option.value)"
            >
              {{ option.label }}
            </button>
          </div>
          <div v-if="current.error || current.loading" class="collect-load" role="status">
            <span>{{ current.error || '正在刷新歌单…' }}</span>
            <button
              v-if="current.error"
              class="text-button"
              :aria-disabled="current.loading || fixedDestination"
              @click="reload()"
            >
              {{ current.loading ? '正在重试…' : '重试' }}
            </button>
          </div>
          <PlaylistCollectList
            :key="sort"
            :playlists="current.rows"
            :disabled="busy || pending || fixedDestination"
            :include-create="!downloadCollection || downloadCollection.canCreate"
            :system-liked="!!likedDestination"
            :show-working="!downloadCollection || busy"
            :selected-id="selected"
            :scroll-top="current.top"
            :loading="current.loading || !!current.error || current.next !== null"
            @scroll="current.top = $event"
            @near-end="reload(true)"
            @select="collect"
            @liked="collectLiked"
            @create="startCreate"
          />
          <button
            v-if="current.next !== null && current.next !== 0 && !current.error"
            type="button"
            class="text-button collect-more"
            :aria-disabled="current.loading || busy || pending || fixedDestination"
            @click="reload()"
          >
            {{ current.loading ? '正在加载…' : '加载更多歌单' }}
          </button>
        </div>
      </BoundedPanelTransition>
    </div>
    <p v-if="busy && !(creating && downloadCollection)" class="collect-status" role="status">
      正在收藏，请稍候…
    </p>
  </dialog>
</template>
