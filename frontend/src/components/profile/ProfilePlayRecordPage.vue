<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue';
import { useCore } from '../../services/core';
import { failure, type TrackView } from '../../services/contracts';
import type {
  ProfilePlayRecordEntry,
  ProfilePlayRecordPeriod,
  ProfilePlayRecordRemovalAction,
  ProfilePlayRecordTarget,
  ProfilePlayRecordView,
} from '../../services/profilePlayRecord';
import Icon from '../Icon.vue';
import ActionMenu from '../ActionMenu.vue';
import type { MenuItem } from '../../services/menu';

const props = defineProps<{
  target: ProfilePlayRecordTarget;
  epoch: string;
  playable: boolean;
  liked: Set<string>;
  busyLikes: Set<string>;
  currentId?: string;
  playing: boolean;
}>();
const emit = defineEmits<{
  period: [value: ProfilePlayRecordPeriod];
  play: [tracks: TrackView[], index: number];
  like: [id: string];
  expired: [];
}>();
const core = useCore();
const view = shallowRef<ProfilePlayRecordView>();
const busy = ref(false);
const failed = ref(false);
const heading = ref<HTMLElement>();
const section = ref<HTMLElement>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const selected = shallowRef<ProfilePlayRecordEntry>();
const removing = ref(false);
const notice = ref('');
const canRemove = computed(
  () => view.value?.state === 'ready' && view.value.isSelf && !!view.value.revision,
);
const menuItems = computed<MenuItem[]>(() =>
  selected.value?.removalUncertain
    ? [{ key: 'check', label: '核对当前榜单' }]
    : [{ key: 'remove', label: '从列表中删除', icon: 'trash', danger: true }],
);
const periodLabel = computed(() => (props.target.period === 'week' ? '最近一周' : '所有时间'));
const items = computed(() => (view.value?.state === 'ready' ? view.value.items : []));
const trackList = computed(() => items.value.map((item) => item.track));
const own = computed(
  () => props.target.uid === null || (view.value?.state === 'ready' && view.value.isSelf),
);
const count = new Intl.NumberFormat('zh-CN');

let revision = 0;
watch([() => props.epoch, () => props.target.uid, () => props.target.period], () => void load(), {
  immediate: true,
  flush: 'sync',
});
onMounted(() => heading.value?.focus({ preventScroll: true }));
onBeforeUnmount(() => revision++);

async function load() {
  const attempt = ++revision;
  const { epoch, target } = props;
  busy.value = true;
  failed.value = false;
  menu.value?.close();
  selected.value = undefined;
  notice.value = '';
  removing.value = false;
  view.value = undefined;
  try {
    const reply = await core.profilePlayRecord(epoch, target.uid, target.period);
    if (attempt !== revision) return;
    if (reply.epoch !== epoch) throw Error('stale profile play record');
    view.value = reply.value;
  } catch (error) {
    if (attempt === revision) {
      const code = failure(error).code;
      if (code === 'session_expired' || code === 'profile_changed') emit('expired');
      failed.value = true;
    }
  } finally {
    if (attempt === revision) busy.value = false;
  }
}
function context(event: MouseEvent | KeyboardEvent, item: ProfilePlayRecordEntry) {
  if (!canRemove.value || removing.value || !(event.currentTarget instanceof HTMLElement)) return;
  const anchor = event.currentTarget;
  if (anchor.closest('[inert]') || document.querySelector('dialog[open], [aria-modal="true"]'))
    return;
  event.preventDefault();
  event.stopPropagation();
  selected.value = item;
  const box = anchor.getBoundingClientRect();
  const pointer = event instanceof MouseEvent && event.button === 2;
  void menu.value?.showAt(
    pointer ? event.clientX : box.left,
    pointer ? event.clientY : box.bottom,
    anchor,
  );
}

function rowKey(event: KeyboardEvent, item: ProfilePlayRecordEntry) {
  if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
    context(event, item);
    return;
  }
  if (
    !canRemove.value ||
    removing.value ||
    event.repeat ||
    event.defaultPrevented ||
    event.altKey ||
    event.ctrlKey ||
    event.metaKey ||
    event.shiftKey ||
    (event.key !== 'Delete' && event.key !== 'Backspace') ||
    event.target !== event.currentTarget ||
    document.activeElement !== event.currentTarget ||
    document.querySelector('dialog[open], [aria-modal="true"], [role="menu"]')
  )
    return;
  event.preventDefault();
  event.stopPropagation();
  if (!item.removalUncertain) void change(item, 'remove');
}

function choose(key: string) {
  if (selected.value && (key === 'remove' || key === 'check')) void change(selected.value, key);
}

async function change(item: ProfilePlayRecordEntry, action: ProfilePlayRecordRemovalAction) {
  const current = view.value;
  if (
    removing.value ||
    current?.state !== 'ready' ||
    !current.isSelf ||
    !current.revision ||
    !current.items.includes(item) ||
    !!item.removalUncertain !== (action === 'check')
  )
    return;
  const ticket = revision;
  const { epoch, target } = props;
  const id = item.track.id;
  const index = current.items.indexOf(item);
  const focused = document.activeElement;
  removing.value = true;
  notice.value = '';
  try {
    const reply = await core.removeProfilePlayRecord(
      epoch,
      target.period,
      id,
      current.revision,
      action,
    );
    if (ticket !== revision || reply.epoch !== epoch) return;
    if (reply.value === 'uncertain') {
      view.value = {
        ...current,
        items: current.items.map((row) =>
          row.track.id === id ? { ...row, removalUncertain: true } : row,
        ),
      };
      notice.value = '删除结果尚未确认，可以核对当前榜单。';
      return;
    }
    view.value = { ...current, items: current.items.filter((row) => row.track.id !== id) };
    notice.value =
      reply.value === 'removed'
        ? `已从我的${periodLabel.value}听歌排行删除该记录。`
        : '此歌曲已不在当前榜单，无法确认之前的删除是否生效。';
    await nextTick();
    if (
      ticket === revision &&
      focused instanceof HTMLElement &&
      !focused.isConnected &&
      document.activeElement === document.body
    ) {
      const rows = section.value?.querySelectorAll<HTMLElement>('[data-rank-song]');
      (rows?.[Math.min(index, rows.length - 1)] ?? heading.value)?.focus({ preventScroll: true });
    }
  } catch (error) {
    if (ticket === revision) {
      const issue = failure(error);
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') emit('expired');
      notice.value = issue.message;
    }
  } finally {
    if (ticket === revision) removing.value = false;
  }
}
</script>

<template>
  <section ref="section" class="listening-rank" aria-label="听歌排行" :aria-busy="busy || removing">
    <h1 ref="heading" tabindex="-1">{{ own ? '我的听歌排行' : `${target.name}的听歌排行` }}</h1>
    <div class="rank-periods" role="group" aria-label="排行时间">
      <button
        v-for="option in [
          { id: 'week', label: '最近一周' },
          { id: 'all', label: '所有时间' },
        ] as const"
        :key="option.id"
        class="text-button"
        type="button"
        :aria-pressed="target.period === option.id"
        @click="target.period !== option.id && emit('period', option.id)"
      >
        {{ option.label }}
      </button>
    </div>
    <p v-if="notice" role="status" data-rank-removal-notice>{{ notice }}</p>
    <p v-if="busy" role="status">正在读取听歌排行…</p>
    <p v-else-if="failed" role="alert">
      无法读取听歌排行
      <button class="text-button" type="button" @click="load">重试</button>
    </p>
    <p v-else-if="view?.state === 'denied'" role="status">此听歌排行不可查看</p>
    <p v-else-if="!items.length" class="muted">暂无听歌排行</p>
    <ol v-else class="rank-items">
      <li
        v-for="(item, index) in items"
        :key="`${item.track.id}:${index}`"
        class="rank-row"
        :data-rank-song="item.track.id"
        :tabindex="canRemove ? 0 : undefined"
        :aria-haspopup="canRemove ? 'menu' : undefined"
        @contextmenu="context($event, item)"
        @keydown="rowKey($event, item)"
      >
        <span aria-hidden="true" class="muted rank-position">{{ index + 1 }}</span>
        <button
          class="rank-song text-button"
          type="button"
          :class="{ 'rank-current': item.track.id === currentId }"
          :disabled="!playable || item.track.missing"
          :aria-label="`${item.track.id === currentId && playing ? '暂停' : '播放'} ${item.track.name}`"
          @click="emit('play', trackList, index)"
        >
          <span class="rank-cover"
            ><img
              v-if="item.track.coverUrl"
              :src="item.track.coverUrl"
              alt=""
              loading="lazy"
              referrerpolicy="no-referrer" /><Icon v-else name="music"
          /></span>
          <span class="rank-meta"
            ><strong>{{ item.track.name }}</strong
            ><span class="muted">{{ item.track.artists.join(' / ') }}</span></span
          >
        </button>
        <button
          class="icon-button"
          :class="{ 'rank-current': liked.has(item.track.id) }"
          type="button"
          :aria-label="`${liked.has(item.track.id) ? '取消喜欢' : '喜欢'} ${item.track.name}`"
          :aria-pressed="liked.has(item.track.id)"
          :disabled="busyLikes.has(item.track.id) || item.track.missing"
          @click="emit('like', item.track.id)"
        >
          <Icon name="heart" />
        </button>
        <button
          v-if="canRemove && item.removalUncertain"
          type="button"
          class="text-button rank-reconcile"
          :aria-label="`核对《${item.track.name}》的删除结果`"
          :disabled="removing"
          @click="change(item, 'check')"
        >
          核对结果
        </button>
        <span v-if="item.playCount !== undefined" class="muted rank-count"
          ><Icon name="play" />
          <span aria-label="播放量">{{ count.format(item.playCount) }}</span></span
        >
      </li>
    </ol>
    <ActionMenu
      v-if="canRemove"
      ref="menu"
      label="听歌排行歌曲操作"
      variant="song"
      context-only
      :items="menuItems"
      :disabled="removing"
      @select="choose"
    >
      <template #item="{ item }">
        <span
          >{{ item.label
          }}<span v-if="item.key === 'remove'" class="sr-only">
            （删除我的{{ periodLabel }}听歌排行记录）
          </span></span
        >
      </template>
    </ActionMenu>
  </section>
</template>

<style scoped>
.rank-periods {
  display: flex;
  gap: 1.25rem;
  margin-block: 1rem;
}
.rank-periods [aria-pressed='true'] {
  color: var(--accent);
  border-bottom: 2px solid currentColor;
  font-weight: 600;
}
.rank-items {
  list-style: none;
  padding: 0;
  margin: 0;
}
.rank-row {
  display: flex;
  min-height: 56px;
  align-items: center;
  gap: 0.625rem;
  padding-inline: 0.625rem;
  border-radius: 6px;
}
.rank-row:nth-child(odd) {
  background: var(--surface-alt);
}
.rank-position {
  width: 2.25rem;
  text-align: center;
  font-variant-numeric: tabular-nums;
}
.rank-song {
  display: flex;
  flex: 1;
  min-width: 0;
  gap: 0.625rem;
  text-align: start;
  color: var(--text);
  align-items: center;
}
.rank-cover {
  display: grid;
  flex: 0 0 36px;
  height: 36px;
  place-items: center;
  border-radius: 4px;
  overflow: hidden;
  background: var(--surface-alt);
}
.rank-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.rank-meta {
  display: grid;
  min-width: 0;
}
.rank-meta > * {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.rank-count {
  display: flex;
  align-items: center;
  justify-content: end;
  min-width: 5rem;
  gap: 0.25rem;
  font-variant-numeric: tabular-nums;
}
.rank-count .icon {
  width: 0.85em;
}
.rank-current {
  color: var(--accent);
}
</style>
