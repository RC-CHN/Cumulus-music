<script setup lang="ts">
import { computed, ref, nextTick, useId } from 'vue';
import type { CloudTrack, CloudQuery } from '../../services/cloud';
import { cloudBytes } from '../../services/cloud';
import { cloudSongColumns } from '../../services/cloudSongs';
import { useCloudSongSelection } from '../../composables/useCloudSongSelection';
import { batchSelectionNeighbor } from '../../services/playlistBatchTable';
import { useTableColumns } from '../../composables/useTableColumns';
import TableColumnResize from '../TableColumnResize.vue';
import Icon from '../Icon.vue';
const props = defineProps<{
  rows: CloudTrack[];
  selected: Set<string>;
  offset: number;
  query: CloudQuery;
  disabled: boolean;
  playable: boolean;
  currentId?: string;
  growth: number[];
}>();
const emit = defineEmits<{
  'update:growth': [value: number[]];
  select: [value: Set<string>];
  sort: [value: Exclude<CloudQuery['sort'], 'natural'>];
  play: [id: string];
  menu: [event: MouseEvent | KeyboardEvent, row: CloudTrack];
  remove: [row: CloudTrack, origin: HTMLElement];
}>();
const root = ref<HTMLElement>(),
  body = ref<HTMLElement>(),
  bodyId = useId(),
  active = ref(0);
const growth = computed({ get: () => props.growth, set: (value) => emit('update:growth', value) });
const columns = useTableColumns(
  root,
  () => cloudSongColumns,
  () => props.disabled,
  growth,
);
const ids = computed(() => props.rows.map((row) => row.track.id));
const selection = useCloudSongSelection(
  () => ids.value,
  () => props.selected,
  (value) => emit('select', value),
);
function sortState(column: CloudQuery['sort']): 'none' | 'ascending' | 'descending' {
  return props.query.sort === column || (column === 'title' && props.query.sort === 'artist')
    ? props.query.descending
      ? 'descending'
      : 'ascending'
    : 'none';
}
function indicator(column: CloudQuery['sort']) {
  return sortState(column) === 'none'
    ? ''
    : `${column === 'title' && props.query.sort === 'artist' ? '歌手 ' : ''}${props.query.descending ? '↓' : '↑'}`;
}
function choose(index: number, ctrl = false, shift = false) {
  if (props.disabled) return;
  selection.choose(index, ctrl, shift);
}
function pointer(event: PointerEvent, index: number) {
  if (
    props.disabled ||
    event.button > 2 ||
    event.button === 1 ||
    (event.target as HTMLElement).closest('button,input')
  )
    return;
  const row = event.currentTarget as HTMLElement;
  if (event.button === 2 && selection.selected.value.has(ids.value[index]!)) return;
  choose(index, event.ctrlKey || event.metaKey, event.shiftKey);
  active.value = index;
  row.focus({ preventScroll: true });
}
function checkbox(index: number, event: MouseEvent) {
  choose(index, !event.shiftKey, event.shiftKey);
}
async function focus(index: number) {
  active.value = Math.max(0, Math.min(props.rows.length - 1, index));
  await nextTick();
  const row = body.value?.querySelector<HTMLElement>(
    `[data-cloud-id="${ids.value[active.value]}"]`,
  );
  row?.focus({ preventScroll: true });
  row?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
}
async function focusBody(event: FocusEvent) {
  if (!event.relatedTarget || props.disabled) return;
  const selected = ids.value.findIndex((id) => selection.selected.value.has(id));
  if (selected < 0) choose(0);
  await focus(Math.max(0, selected));
}
async function key(event: KeyboardEvent, index = active.value) {
  const row = (event.currentTarget as HTMLElement).closest('[data-cloud-id]');
  if (!event.altKey && (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey))) {
    if (row) emit('menu', event, props.rows[index]!);
    return;
  }
  if (
    event.altKey ||
    (event.target !== event.currentTarget &&
      (event.target as HTMLElement).closest('button,input,[role=separator]') &&
      !(
        (event.target as HTMLElement).matches('input[type=checkbox]') &&
        ![' ', 'Enter'].includes(event.key)
      ))
  )
    return;
  if (props.disabled) return;
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
    event.preventDefault();
    event.stopPropagation();
    selection.update(new Set(ids.value));
    return;
  }
  if (['Delete', 'Backspace'].includes(event.key)) {
    event.preventDefault();
    event.stopPropagation();
    if (!event.repeat && row) emit('remove', props.rows[index]!, row as HTMLElement);
    return;
  }
  if (event.key === 'Enter') {
    event.preventDefault();
    event.stopPropagation();
    if (props.playable && !event.repeat) emit('play', ids.value[index]!);
    return;
  }
  if (event.key === ' ') {
    event.preventDefault();
    event.stopPropagation();
    choose(index, true, event.shiftKey);
    return;
  }
  let target: number | undefined;
  if (event.key === 'Home') target = 0;
  else if (event.key === 'End') target = ids.value.length - 1;
  else if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
    const direction = event.key === 'ArrowUp' ? -1 : 1;
    target =
      event.shiftKey || event.ctrlKey || event.metaKey
        ? index + direction
        : batchSelectionNeighbor(ids.value, selection.selected.value, direction);
  } else return;
  event.preventDefault();
  event.stopPropagation();
  if (target == null) return;
  target = Math.max(0, Math.min(ids.value.length - 1, target));
  if (event.shiftKey) choose(target, event.ctrlKey || event.metaKey, true);
  else if (!event.ctrlKey && !event.metaKey) choose(target);
  await focus(target);
}
</script>
<template>
  <div ref="root" class="cloud-table-wrap">
    <table class="cloud-table" aria-label="已上传云盘歌曲">
      <colgroup>
        <col
          v-for="(width, index) in columns.widths.value"
          :key="index"
          :style="{ width: `${width}px` }"
        />
      </colgroup>
      <thead>
        <tr>
          <th class="cloud-check">
            <input
              type="checkbox"
              aria-label="选择本页全部歌曲"
              :checked="selected.size === ids.length && !!ids.length"
              :indeterminate="selected.size > 0 && selected.size < ids.length"
              @change="selection.all"
              :disabled="disabled"
            />
          </th>
          <th class="cloud-index">#</th>
          <th :aria-sort="sortState('title')">
            <button :disabled="disabled" data-cloud-sort="title" @click="emit('sort', 'title')">
              标题 <span>{{ indicator('title') }}</span></button
            ><TableColumnResize :model="columns" :column="2" label="标题" :controls="bodyId" />
          </th>
          <th :aria-sort="sortState('album')">
            <button :disabled="disabled" data-cloud-sort="album" @click="emit('sort', 'album')">
              专辑 <span>{{ indicator('album') }}</span></button
            ><TableColumnResize :model="columns" :column="3" label="专辑" :controls="bodyId" />
          </th>
          <th :aria-sort="sortState('format')">
            <button :disabled="disabled" data-cloud-sort="format" @click="emit('sort', 'format')">
              格式 <span>{{ indicator('format') }}</span>
            </button>
          </th>
          <th :aria-sort="sortState('uploaded')">
            <button
              :disabled="disabled"
              data-cloud-sort="uploaded"
              @click="emit('sort', 'uploaded')"
            >
              上传时间 <span>{{ indicator('uploaded') }}</span></button
            ><TableColumnResize :model="columns" :column="5" label="上传时间" :controls="bodyId" />
          </th>
          <th :aria-sort="sortState('size')">
            <button :disabled="disabled" data-cloud-sort="size" @click="emit('sort', 'size')">
              大小 <span>{{ indicator('size') }}</span>
            </button>
          </th>
        </tr>
      </thead>
      <tbody
        ref="body"
        :id="bodyId"
        tabindex="0"
        @focus.self="focusBody"
        @keydown.self="key($event)"
      >
        <tr
          v-for="(row, index) in rows"
          :key="row.track.id"
          :data-cloud-id="row.track.id"
          :aria-selected="selected.has(row.track.id)"
          tabindex="-1"
          @focus="active = index"
          @pointerdown="pointer($event, index)"
          :class="{ selected: selected.has(row.track.id), playing: currentId === row.track.id }"
          @contextmenu="emit('menu', $event, row)"
          @keydown="key($event, index)"
          @dblclick="
            !($event.target as HTMLElement).closest('button,input') && emit('play', row.track.id)
          "
        >
          <td>
            <input
              type="checkbox"
              :aria-label="`选择 ${row.track.name}`"
              :checked="selected.has(row.track.id)"
              @click.stop="checkbox(index, $event)"
              :disabled="disabled"
              tabindex="-1"
            />
          </td>
          <td>
            <button
              class="cloud-row-play"
              :aria-label="`播放 ${row.track.name}`"
              :disabled="!playable || disabled"
              @click="emit('play', row.track.id)"
            >
              <span>{{ offset + index + 1 }}</span
              ><Icon name="play" />
            </button>
          </td>
          <td class="cloud-track-title">
            <div class="cloud-title-content">
              <div class="cloud-title-copy">
                <strong :title="row.track.name">{{ row.track.name }}</strong
                ><span
                  ><span class="cloud-artist" :title="row.track.artists.join(' / ')">{{
                    row.track.artists.join(' / ') || '未知歌手'
                  }}</span
                  ><small v-if="row.bitrate >= 900000">无损</small></span
                >
              </div>
              <button
                class="icon-button cloud-more"
                :aria-label="`更多操作：${row.track.name}`"
                aria-haspopup="menu"
                :disabled="disabled"
                @click="emit('menu', $event, row)"
              >
                <Icon name="more" />
              </button>
            </div>
          </td>
          <td class="cloud-album" :title="row.track.album">
            {{ row.track.album || '未知专辑' }}
          </td>
          <td class="cloud-format">{{ row.format.toUpperCase() || '—' }}</td>
          <td class="cloud-date">
            {{ row.addedAt ? new Date(row.addedAt).toLocaleDateString('zh-CN') : '—' }}
          </td>
          <td class="cloud-size">{{ cloudBytes(row.fileSize) }}</td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
