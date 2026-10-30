<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import type { DownloadTask } from '../../services/downloads';
import { artworkAt } from '../../services/artwork';
import { time } from '../../services/player';
import { useVirtualRows } from '../../composables/useVirtualRows';
import { useDownloadMvFocus } from '../../composables/useDownloadMvFocus';
import ActionMenu from '../ActionMenu.vue';
import Icon from '../Icon.vue';
import DownloadContinuation from './DownloadContinuation.vue';
const props = defineProps<{
  tasks: DownloadTask[];
  more: boolean;
  loadingMore: boolean;
  moreError: string;
}>();
const emit = defineEmits<{
  play: [task: DownloadTask];
  manage: [task: DownloadTask];
  artist: [id: string];
  load: [];
}>();
const width = ref(836),
  columns = computed(() => (width.value < 600 ? 2 : 4));
const rowHeight = computed(
  () => Math.ceil(((width.value - (columns.value - 1) * 24) / columns.value) * 0.565) + 88,
);
const total = computed(() => Math.ceil(props.tasks.length / columns.value));
const { container, start, end, onScroll } = useVirtualRows(
  () => total.value,
  () => rowHeight.value,
  2,
);
const focus = useDownloadMvFocus(
  () => props.tasks,
  container,
  columns,
  rowHeight,
  start,
  end,
  onScroll,
);
const menu = ref<InstanceType<typeof ActionMenu>>(),
  menuId = ref('');
let menuOrigin: HTMLElement | undefined,
  panelId = '';
function scrollGrid() {
  // ActionMenu dismisses on outside scroll. If focus was inside that menu,
  // hand it to its visible card or to the grid before the origin is recycled.
  const grid = container.value;
  if (grid && panelId && document.activeElement?.closest('.action-menu')?.id === panelId) {
    const b = grid.getBoundingClientRect(),
      r = menuOrigin?.getBoundingClientRect();
    if (r && menuOrigin?.isConnected && r.top >= b.top && r.bottom <= b.bottom)
      menuOrigin.focus({ preventScroll: true });
    else grid.focus({ preventScroll: true });
  }
  onScroll();
}
const actions = [
  { key: 'play', label: '播放 MV', icon: 'play' },
  { key: 'file', label: '管理下载文件…', icon: 'more' },
];
async function context(event: MouseEvent | KeyboardEvent, task: DownloadTask) {
  if (
    event instanceof KeyboardEvent &&
    !(event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey))
  )
    return;
  event.preventDefault();
  event.stopPropagation();
  const card = (event.currentTarget as HTMLElement).closest<HTMLElement>('[data-download-mv]');
  const origin =
    (event.target as HTMLElement).closest<HTMLElement>('button') ??
    card?.querySelector<HTMLElement>('.download-mv-cover');
  if (!origin) return;
  origin.focus({ preventScroll: true });
  menuId.value = task.id;
  await nextTick();
  if (!origin.isConnected) return;
  const r = origin.getBoundingClientRect(),
    mouse = event instanceof MouseEvent && event.type === 'contextmenu' && event.button === 2;
  menuOrigin = origin;
  await menu.value?.showAt(
    mouse ? event.clientX : r.left,
    mouse ? event.clientY : r.bottom,
    origin,
  );
  panelId = origin.getAttribute('aria-controls') ?? '';
}
function choose(key: string) {
  const task = props.tasks.find((t) => t.id === menuId.value);
  if (task) {
    if (key === 'play') emit('play', task);
    else if (key === 'file') emit('manage', task);
  }
}
watch(
  () => props.tasks,
  () => {
    if (menuId.value && !props.tasks.some((t) => t.id === menuId.value)) menu.value?.close(true);
  },
);
watch(
  [end, total, () => props.more, () => props.loadingMore],
  () => {
    if (end.value >= total.value && props.more && !props.loadingMore && !props.moreError)
      emit('load');
  },
  { flush: 'post' },
);
let observer: ResizeObserver | undefined;
watch(
  container,
  (element) => {
    observer?.disconnect();
    if (element) {
      width.value = element.clientWidth;
      observer = new ResizeObserver(() => {
        width.value = element.clientWidth;
      });
      observer.observe(element);
    }
  },
  { flush: 'post' },
);
onBeforeUnmount(() => observer?.disconnect());
</script>
<template>
  <div
    ref="container"
    class="download-mv-scroll"
    :data-download-columns="columns"
    :data-download-pitch="rowHeight"
    tabindex="-1"
    role="region"
    aria-label="已下载MV"
    @focusin="focus.focusIn"
    @keydown="focus.key"
    @scroll.passive="scrollGrid"
  >
    <div :style="{ height: `${start * rowHeight}px` }" aria-hidden="true" />
    <div
      v-for="index in end - start"
      :key="start + index"
      :data-mv-row="start + index - 1"
      class="download-mv-row"
      :style="{
        gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
        height: `${rowHeight}px`,
      }"
    >
      <article
        v-for="(task, offset) in tasks.slice(
          (start + index - 1) * columns,
          (start + index) * columns,
        )"
        :key="task.id"
        class="download-mv-card"
        :data-download-mv="task.id"
        :data-mv-index="(start + index - 1) * columns + offset"
        @contextmenu="context($event, task)"
        @keydown="context($event, task)"
        @click.self="emit('play', task)"
      >
        <button
          class="download-mv-cover"
          data-download-action="cover"
          :tabindex="focus.roving.value === task.id ? 0 : -1"
          aria-haspopup="menu"
          :aria-label="`播放下载MV ${task.track.name}`"
          @click="emit('play', task)"
        >
          <img
            v-if="task.track.coverUrl"
            :src="artworkAt(task.track.coverUrl, 480)"
            alt=""
            loading="lazy"
            decoding="async"
            referrerpolicy="no-referrer"
          /><Icon v-else name="video" class="download-mv-placeholder" /><Icon
            name="play"
            class="download-mv-play"
          /><span class="download-mv-resolution">{{
            (task.actualResolution ?? task.requestedResolution)
              ? (task.actualResolution ?? task.requestedResolution) + 'P'
              : '—'
          }}</span
          ><span class="download-mv-duration">{{ time(task.track.durationMs / 1000) }}</span>
        </button>
        <div class="download-mv-info" @click.self="emit('play', task)">
          <button
            class="download-mv-title ellipsis"
            data-download-action="title"
            tabindex="-1"
            :data-tooltip="task.track.name"
            @click="emit('play', task)"
          >
            {{ task.track.name }}</button
          ><span
            class="ellipsis muted download-mv-artists"
            :data-tooltip="task.track.artists.join(' / ')"
          >
            <template v-if="task.track.artistRefs?.length">
              <template v-for="(artist, i) in task.track.artistRefs" :key="i">
                <span v-if="i"> / </span>
                <button
                  v-if="artist.id"
                  :data-download-action="`artist:${artist.id}`"
                  :tabindex="focus.roving.value === task.id ? 0 : -1"
                  @click.stop="emit('artist', artist.id)"
                >
                  {{ artist.name }}</button
                ><span v-else>{{ artist.name }}</span>
              </template> </template
            ><template v-else>{{ task.track.artists.join(' / ') }}</template> </span
          ><button
            class="icon-button download-mv-manage"
            data-download-action="manage"
            aria-haspopup="menu"
            :tabindex="focus.roving.value === task.id ? 0 : -1"
            :aria-label="`管理下载 ${task.track.name}`"
            @click="context($event, task)"
          >
            <Icon name="more" />
          </button>
        </div>
      </article>
    </div>
    <div :style="{ height: `${(total - end) * rowHeight}px` }" aria-hidden="true" />
    <ActionMenu ref="menu" label="下载MV操作" context-only :items="actions" @select="choose" />
    <DownloadContinuation
      :more="more"
      :loading="loadingMore"
      :error="moreError"
      @load="emit('load')"
    />
  </div>
</template>
