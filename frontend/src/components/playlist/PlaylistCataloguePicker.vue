<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import type { usePlaylistTagCatalogue } from '../../composables/usePlaylistTagCatalogue';
import Icon from '../Icon.vue';

const props = defineProps<{ tag: string; model: ReturnType<typeof usePlaylistTagCatalogue> }>();
const emit = defineEmits<{ choose: [tag: string] }>();
const { groups, pending, error, load } = props.model;
const open = ref(false);
const groupId = ref<number>();
const trigger = ref<HTMLButtonElement>();
const panel = ref<HTMLElement>();
const root = ref<HTMLElement>();
const group = computed(() => groups.value?.find((entry) => entry.id === groupId.value));
watch([open, groups, () => props.tag], () => {
  if (!open.value || !groups.value) return;
  groupId.value =
    groups.value.find((entry) => entry.tags.some((tag) => tag.name === props.tag))?.id ??
    groups.value[0]?.id;
});
async function toggle() {
  if (open.value) return close(true);
  open.value = true;
  void load();
  await nextTick();
  panel.value?.focus({ preventScroll: true });
}
function close(restore: boolean) {
  open.value = false;
  if (restore) trigger.value?.focus({ preventScroll: true });
}
function choose(tag: string) {
  if (open.value) close(true);
  if (tag !== props.tag) emit('choose', tag);
}
function retry() {
  panel.value?.focus({ preventScroll: true });
  void load();
}
function escape(event: KeyboardEvent) {
  if (!open.value) return;
  event.stopPropagation();
  close(true);
}
function pointerOutside(event: PointerEvent) {
  if (open.value && event.target instanceof Node && !root.value?.contains(event.target))
    close(false);
}
function focusOutside(event: FocusEvent) {
  if (
    open.value &&
    event.relatedTarget instanceof Node &&
    !root.value?.contains(event.relatedTarget)
  )
    close(false);
}
onMounted(() => document.addEventListener('pointerdown', pointerOutside, true));
onUnmounted(() => document.removeEventListener('pointerdown', pointerOutside, true));
</script>

<template>
  <div ref="root" class="playlist-catalogue" @keydown.esc="escape" @focusout="focusOutside">
    <div class="playlist-catalogue-controls" role="group" aria-label="歌单分类">
      <button class="secondary" :aria-pressed="tag === '推荐'" @click="choose('推荐')">推荐</button>
      <button class="secondary" :aria-pressed="tag === '官方'" @click="choose('官方')">官方</button>
      <button
        ref="trigger"
        class="secondary playlist-catalogue-trigger"
        :aria-expanded="open"
        aria-controls="playlist-catalogue-panel"
        @click="toggle"
      >
        更多标签 <Icon name="down" />
      </button>
    </div>
    <section
      v-if="open"
      id="playlist-catalogue-panel"
      ref="panel"
      class="playlist-catalogue-panel"
      aria-label="歌单标签"
      tabindex="-1"
    >
      <p v-if="pending" class="muted" role="status">正在读取歌单标签…</p>
      <div v-else-if="error" role="status">
        <p>{{ error }}</p>
        <button class="secondary" @click="retry">重新加载</button>
      </div>
      <p v-else-if="groups && !groups.length" class="muted">暂无可选歌单标签。</p>
      <template v-else-if="groups">
        <div class="playlist-catalogue-groups" role="group" aria-label="标签分组">
          <button
            v-for="entry in groups"
            :key="entry.id"
            class="text-button"
            :aria-pressed="entry.id === groupId"
            @click="groupId = entry.id"
          >
            {{ entry.name }}
          </button>
        </div>
        <div v-if="group" class="playlist-catalogue-tags" role="group" :aria-label="group.name">
          <button
            v-for="entry in group.tags"
            :key="entry.name"
            :aria-pressed="entry.name === tag"
            @click="choose(entry.name)"
          >
            {{ entry.displayName ?? entry.name }}
          </button>
        </div>
      </template>
    </section>
  </div>
</template>

<style scoped>
.playlist-catalogue {
  position: relative;
  margin-bottom: 16px;
}
.playlist-catalogue-controls,
.playlist-catalogue-groups {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.playlist-catalogue-controls .icon {
  width: 14px;
  height: 14px;
}
.playlist-catalogue-panel {
  position: absolute;
  z-index: 6;
  top: calc(100% + 4px);
  left: 0;
  box-sizing: border-box;
  width: min(620px, 100%);
  max-height: min(48vh, 360px);
  overflow: auto;
  padding: 16px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--surface);
  box-shadow: 0 8px 26px rgb(0 0 0 / 14%);
}
.playlist-catalogue-groups {
  border-bottom: 1px solid var(--line);
  padding-bottom: 6px;
}
.playlist-catalogue-groups button {
  position: relative;
  padding: 4px 8px;
  color: var(--text);
}
.playlist-catalogue-tags {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(100%, 80px), 1fr));
  gap: 12px;
  margin-top: 16px;
}
.playlist-catalogue-tags button {
  min-width: 0;
  padding: 6px;
  border: 1px solid var(--line);
  border-radius: 20px;
  overflow-wrap: anywhere;
}
.playlist-catalogue button[aria-pressed='true'] {
  color: var(--accent);
  background: var(--surface-hover);
}
.playlist-catalogue-groups button[aria-pressed='true'] {
  background: transparent;
}
.playlist-catalogue-groups button[aria-pressed='true']::after {
  position: absolute;
  right: 8px;
  bottom: -7px;
  left: 8px;
  height: 2px;
  background: var(--accent);
  content: '';
}
</style>
