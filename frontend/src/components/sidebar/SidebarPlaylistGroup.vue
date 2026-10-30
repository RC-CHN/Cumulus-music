<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import type { PlaylistView } from '../../services/contracts';
import type { MenuItem } from '../../services/menu';
import { useOuterVirtualRows } from '../../composables/useOuterVirtualRows';
import Icon from '../Icon.vue';
import ActionMenu from '../ActionMenu.vue';
import SidebarPlaylistRow from './SidebarPlaylistRow.vue';
const props = defineProps<{
  group: 'created' | 'subscribed';
  playlists: PlaylistView[];
  expanded: boolean;
  scroller?: HTMLElement;
  selectedId?: string;
  playingId?: string;
  items: (list: PlaylistView) => MenuItem[];
}>();
const emit = defineEmits<{
  toggle: [];
  create: [];
  open: [list: PlaylistView];
  action: [list: PlaylistView, key: string];
}>();
const title = computed(() => (props.group === 'created' ? '创建的歌单' : '收藏的歌单'));
const { container, start, end, update } = useOuterVirtualRows(
  () => props.playlists.length,
  46,
  () => props.scroller,
);
const active = ref(0);
const heading = ref<HTMLButtonElement>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const target = ref<PlaylistView>();
const menuItems = computed(() => (target.value ? props.items(target.value) : []));
watch(
  () => props.playlists,
  (now, before) => {
    const id = before[active.value]?.id;
    active.value = Math.max(
      0,
      now.findIndex((list) => list.id === id),
    );
    if (target.value && !now.some((list) => list.id === target.value?.id)) {
      menu.value?.close();
      target.value = undefined;
    }
  },
);
function toggle() {
  menu.value?.close();
  heading.value?.focus({ preventScroll: true });
  emit('toggle');
}
async function focusRow(index: number) {
  const root = props.scroller,
    list = container.value;
  if (!root || !list || !props.playlists.length) return;
  active.value = Math.max(0, Math.min(props.playlists.length - 1, index));
  const top =
    list.getBoundingClientRect().top -
    root.getBoundingClientRect().top +
    root.scrollTop +
    4 +
    active.value * 46;
  if (top < root.scrollTop) root.scrollTop = top;
  else if (top + 46 > root.scrollTop + root.clientHeight)
    root.scrollTop = top + 46 - root.clientHeight;
  update();
  await nextTick();
  list
    .querySelector<HTMLElement>(`[data-sidebar-index="${active.value}"]`)
    ?.focus({ preventScroll: true });
}
function keyboard(event: KeyboardEvent) {
  if (event.key === 'ArrowLeft') {
    event.preventDefault();
    toggle();
    return;
  }
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  const visibleStart =
    props.scroller && container.value
      ? Math.max(
          0,
          Math.floor(
            (props.scroller.getBoundingClientRect().top -
              container.value.getBoundingClientRect().top -
              4) /
              46,
          ),
        )
      : start.value;
  const index = document.activeElement === container.value ? visibleStart : active.value;
  void focusRow(
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? props.playlists.length - 1
        : index + (event.key === 'ArrowDown' ? 1 : -1),
  );
}
async function context(event: MouseEvent | KeyboardEvent, list: PlaylistView) {
  if (
    event instanceof KeyboardEvent &&
    !(event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey))
  )
    return;
  if (!props.items(list).length) return;
  event.preventDefault();
  event.stopPropagation();
  const trigger = event.currentTarget as HTMLElement;
  target.value = list;
  await nextTick();
  const r = trigger.getBoundingClientRect();
  const mouse = event instanceof MouseEvent && event.button === 2;
  await menu.value?.showAt(
    mouse ? event.clientX : r.left,
    mouse ? event.clientY : r.bottom,
    trigger,
  );
}
</script>
<template>
  <section class="sidebar-playlist-group" :data-sidebar-group="group" :aria-label="title">
    <header>
      <button
        ref="heading"
        class="sidebar-group-toggle"
        :aria-expanded="expanded"
        :aria-controls="`sidebar-${group}`"
        :aria-label="`${expanded ? '收起' : '展开'}${title}`"
        @click="toggle"
        @keydown.left.prevent="expanded && toggle()"
        @keydown.right.prevent="expanded ? focusRow(0) : toggle()"
      >
        <span>{{ title }} {{ playlists.length > 999 ? '999+' : playlists.length }}</span>
        <Icon v-if="playlists.length" name="down" :class="{ folded: !expanded }" />
      </button>
      <button
        v-if="group === 'created'"
        class="sidebar-create"
        aria-label="新建歌单"
        data-tooltip="新建歌单"
        @click="emit('create')"
      >
        <Icon name="plus" />
      </button>
    </header>
    <Transition name="sidebar-fold">
      <div
        v-if="expanded && playlists.length"
        :id="`sidebar-${group}`"
        ref="container"
        class="sidebar-playlist-list"
        :tabindex="active < start || active >= end ? 0 : -1"
        :aria-label="title"
        @keydown="keyboard"
      >
        <div :style="{ height: `${start * 46}px` }" aria-hidden="true" />
        <button
          v-for="(list, offset) in playlists.slice(start, end)"
          :key="list.id"
          class="playlist-nav"
          :class="{ selected: selectedId === list.id }"
          :data-sidebar-id="list.id"
          :data-sidebar-index="start + offset"
          :tabindex="active === start + offset ? 0 : -1"
          :aria-current="selectedId === list.id ? 'page' : undefined"
          :aria-label="`${list.name}${list.private ? '，私密歌单' : ''}${playingId === list.id ? '，正在播放' : ''}`"
          :data-tooltip="list.name"
          aria-haspopup="menu"
          @focus="active = start + offset"
          @click="emit('open', list)"
          @contextmenu="context($event, list)"
          @keydown="context($event, list)"
        >
          <SidebarPlaylistRow
            :playlist="list"
            :selected="selectedId === list.id"
            :playing="playingId === list.id"
          />
        </button>
        <div :style="{ height: `${(playlists.length - end) * 46}px` }" aria-hidden="true" />
      </div>
    </Transition>
    <ActionMenu
      ref="menu"
      context-only
      :label="`${title}操作`"
      :items="menuItems"
      @select="target && emit('action', target, $event)"
    />
  </section>
</template>
