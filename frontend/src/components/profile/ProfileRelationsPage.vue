<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import type { useProfileRelations } from '../../composables/useProfileRelations';
import { useVirtualRows } from '../../composables/useVirtualRows';
import type {
  ProfileRelationCard,
  ProfileRelationKind,
  ProfileRelationTarget,
} from '../../services/profileRelations';
import Icon from '../Icon.vue';

const props = defineProps<{
  model: ReturnType<typeof useProfileRelations>;
  target: ProfileRelationTarget;
}>();
const emit = defineEmits<{ open: [card: ProfileRelationCard] }>();
const filters = [
  ['following', '全部'],
  ['following_artists', '歌手'],
  ['following_users', '用户'],
] as const;
const heading = ref<HTMLElement>();
const columns = ref(2);
const rowHeight = 100;
const page = computed(() => props.model.list.value);
const permission = computed(() => props.model.visit.value?.permission);
const items = computed(() => (permission.value === 'ready' ? (page.value?.items ?? []) : []));
const rowCount = computed(() => Math.ceil(items.value.length / columns.value));
const { container, start, end, onScroll, restore } = useVirtualRows(
  () => rowCount.value,
  rowHeight,
  2,
);
const visible = computed(() =>
  items.value.slice(start.value * columns.value, end.value * columns.value),
);
const title = computed(() =>
  props.target.isSelf && props.target.mode === 'following'
    ? '我的关注'
    : `${props.target.name}的${props.target.mode === 'following' ? '关注' : '粉丝'}`,
);
const failedArt = ref(new Set<string>());
const artKey = (card: ProfileRelationCard) => `${card.kind}:${card.id}`;
let observer: ResizeObserver | undefined;
watch(
  container,
  (element) => {
    observer?.disconnect();
    if (!element) return;
    observer = new ResizeObserver(([entry]) => {
      const width = entry?.contentRect.width ?? 0;
      columns.value = width >= 1030 ? 4 : width >= 760 ? 3 : width >= 520 ? 2 : 1;
    });
    observer.observe(element);
  },
  { flush: 'post' },
);
onUnmounted(() => observer?.disconnect());

function focusSaved(index: number | null | undefined) {
  if (index == null) return false;
  const button = container.value?.querySelector<HTMLElement>(`[data-relation-index="${index}"]`);
  if (!button) return false;
  button.focus({ preventScroll: true });
  return true;
}

onMounted(async () => {
  const origin = document.activeElement;
  await nextTick();
  if (!container.value?.isConnected || container.value.closest('[inert]')) return;
  restore(page.value?.top ?? 0);
  await nextTick();
  if (document.activeElement !== document.body && document.activeElement !== origin) return;
  if (!focusSaved(page.value?.focus)) heading.value?.focus({ preventScroll: true });
});

watch(permission, async (now, prior) => {
  if (
    now !== 'ready' ||
    prior !== 'checking' ||
    props.model.visit.value?.target.relationVisit !== props.target.relationVisit
  )
    return;
  await nextTick();
  restore(page.value?.top ?? 0);
  await nextTick();
  if (document.activeElement === heading.value || document.activeElement === document.body)
    focusSaved(page.value?.focus);
});

async function select(kind: ProfileRelationKind) {
  props.model.remember(container.value?.scrollTop ?? 0);
  props.model.select(kind);
  await nextTick();
  restore(page.value?.top ?? 0);
}

function scroll() {
  onScroll();
  const element = container.value;
  if (!element) return;
  props.model.remember(element.scrollTop);
  if (
    element.scrollTop > 0 &&
    element.scrollHeight - element.scrollTop - element.clientHeight < 160 &&
    page.value?.continuation &&
    !page.value.failed
  )
    void props.model.load();
}

function open(card: ProfileRelationCard, index: number) {
  props.model.remember(container.value?.scrollTop ?? 0, index);
  emit('open', card);
}
</script>

<template>
  <section
    class="profile-relations"
    :aria-label="title"
    :aria-busy="page?.busy || permission === 'checking'"
    :data-mode="target.mode"
  >
    <header class="relations-header">
      <h1 ref="heading" tabindex="-1">{{ title }}</h1>
      <div
        v-if="target.mode === 'following' && target.isSelf"
        class="relations-filters"
        role="group"
        aria-label="我的关注分类"
      >
        <button
          v-for="[kind, label] in filters"
          :key="kind"
          type="button"
          class="text-button"
          :data-relation-filter="kind"
          :aria-pressed="model.visit.value?.kind === kind"
          @click="select(kind)"
        >
          {{ label }}
        </button>
      </div>
    </header>
    <div ref="container" class="relations-scroll" @scroll.passive="scroll">
      <p v-if="model.denied.value" role="alert">{{ model.denied.value }}</p>
      <div :style="{ height: `${start * rowHeight}px` }" aria-hidden="true" />
      <div
        class="relations-grid"
        role="list"
        :aria-label="title"
        :style="{ gridTemplateColumns: `repeat(${columns},minmax(0,1fr))` }"
      >
        <div
          v-for="(card, offset) in visible"
          :key="`${start * columns + offset}:${artKey(card)}`"
          class="relation-item"
          role="listitem"
        >
          <button
            type="button"
            class="relation-card"
            :data-relation-resource="artKey(card)"
            :data-relation-index="start * columns + offset"
            :aria-label="`查看${card.kind === 'artist' ? '歌手' : '用户'} ${card.remark || card.name}`"
            @click="open(card, start * columns + offset)"
          >
            <span class="relation-avatar">
              <img
                v-if="card.avatarUrl && !failedArt.has(artKey(card))"
                :src="card.avatarUrl"
                alt=""
                loading="lazy"
                referrerpolicy="no-referrer"
                @error="failedArt.add(artKey(card))"
              />
              <Icon v-else name="user" />
            </span>
            <span class="relation-copy">
              <span class="relation-name" :title="card.name">
                <template v-if="card.remark"
                  >{{ card.remark }} <small>({{ card.name }})</small></template
                >
                <template v-else>{{ card.name }}</template>
              </span>
              <span v-if="card.signature" class="relation-signature" :title="card.signature">{{
                card.signature
              }}</span>
              <span v-if="card.relationship" class="relation-status">{{
                card.relationship === 'mutual' ? '已互关' : '已关注'
              }}</span>
            </span>
          </button>
        </div>
      </div>
      <div :style="{ height: `${(rowCount - end) * rowHeight}px` }" aria-hidden="true" />
      <p v-if="permission === 'checking'" role="status">正在重新确认查看权限…</p>
      <div v-else-if="permission === 'failed'" class="relations-state" role="alert">
        <span>无法确认查看权限</span>
        <button type="button" class="text-button" @click="model.recheck">重试</button>
      </div>
      <p v-else-if="page?.busy" role="status">
        正在读取{{ target.mode === 'following' ? '关注' : '粉丝' }}…
      </p>
      <div v-else-if="page?.failed" class="relations-state" role="alert">
        <span>无法读取{{ target.mode === 'following' ? '关注' : '粉丝' }}列表</span>
        <button type="button" class="text-button" data-relation-retry @click="model.retry">
          {{ page.stale ? '重新加载' : '重试' }}
        </button>
      </div>
      <p
        v-else-if="page?.loaded && !items.length && !page.continuation"
        class="muted"
        role="status"
      >
        {{ target.mode === 'following' ? '暂无关注' : '暂无粉丝' }}
      </p>
      <button
        v-if="permission === 'ready' && page?.continuation && !page.busy && !page.failed"
        type="button"
        class="text-button relations-more"
        data-relation-more
        @click="model.load"
      >
        加载更多
      </button>
    </div>
  </section>
</template>

<style scoped>
.profile-relations {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  min-width: 0;
}
.relations-header {
  flex: none;
  margin: 8px 0 14px;
}
.relations-header h1 {
  margin: 0 0 14px;
  line-height: 31px;
}
.relations-filters {
  display: flex;
  gap: 12px;
  border-bottom: 1px solid var(--line);
  padding: 0 0 8px;
}
.relations-filters button[aria-pressed='true'] {
  color: var(--accent);
  font-weight: 600;
}
.relations-scroll {
  min-height: 0;
  flex: 1;
  overflow: auto;
  scrollbar-gutter: stable;
}
.relations-grid {
  display: grid;
  grid-auto-rows: 100px;
  gap: 0 10px;
}
.relation-item {
  min-width: 0;
  padding-bottom: 12px;
}
.relation-card {
  display: flex;
  align-items: center;
  width: 100%;
  height: 100%;
  min-width: 0;
  gap: 12px;
  padding: 8px;
  text-align: left;
  border: 0;
  border-radius: 14px;
  background: transparent;
  color: var(--text);
  cursor: pointer;
}
.relation-card:hover {
  background: var(--surface-alt);
}
.relation-card:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
.relation-avatar {
  display: grid;
  flex: 0 0 64px;
  block-size: 64px;
  place-items: center;
  overflow: hidden;
  border-radius: 50%;
  background: var(--surface-alt);
}
.relation-avatar img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.relation-copy {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 3px;
}
.relation-name,
.relation-signature,
.relation-status {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.relation-name {
  font-size: 16px;
  line-height: 20px;
}
.relation-name small,
.relation-signature,
.relation-status {
  color: var(--muted);
  font-size: 12px;
}
.relations-more {
  display: block;
  margin: 6px auto 18px;
}
.relations-state {
  display: flex;
  align-items: center;
  gap: 8px;
}
</style>
