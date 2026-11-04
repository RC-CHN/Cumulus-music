<script setup lang="ts">
import { computed, inject, nextTick, ref, watch } from 'vue';
import type { CommunityCard, CommunityResource } from '../../services/community';
import { communityDeleteKey } from '../../composables/useCommunityDelete';
import { communityReportKey } from '../../composables/useCommunityReport';
import { profileReportChoices, type ProfileReportChoice } from '../../services/profileReport';
import Icon from '../Icon.vue';
import CommunityTopicText from './CommunityTopicText.vue';

const props = defineProps<{
  card: CommunityCard;
  nested?: boolean;
  playable: boolean;
  sourceKey?: string;
}>();
const emit = defineEmits<{
  open: [id: string];
  resource: [value: CommunityResource];
  topic: [name: string, opener: string];
}>();
const deletion = inject(communityDeleteKey, null);
const reporter = inject(communityReportKey, null);
const mayDelete = computed(() => !!deletion && !props.nested && !!props.card.canDelete);
const mayReport = computed(
  () => !!reporter && !props.nested && !!props.card.canReport && !props.card.canDelete,
);
const menu = ref(false);
const reasons = ref(false);
const menuOpener = ref<HTMLButtonElement>();
const primaryItem = ref<HTMLButtonElement>();
const reasonList = ref<HTMLElement>();
const popup = ref<HTMLElement>();
const anchor = ref({ left: 0, top: 0, offset: 0, flyoutLeft: true });
const anchorStyle = computed(() => ({
  left: `${anchor.value.left}px`,
  top: `${anchor.value.top}px`,
}));
const reasonStyle = computed(() => ({ top: `${anchor.value.offset}px` }));
let triggerPosition: DOMRect | undefined;
function positionMenu(event?: MouseEvent) {
  const rect = menuOpener.value?.getBoundingClientRect();
  if (!rect) return;
  triggerPosition = rect;
  const contextual = event?.type === 'contextmenu' && (event.clientX || event.clientY);
  const x = contextual ? event.clientX : rect.right - 112;
  const y = contextual ? event.clientY : rect.bottom + 4;
  const left = Math.max(8, Math.min(x, innerWidth - 120));
  const top = Math.max(8, Math.min(y, innerHeight - 56));
  anchor.value = {
    left,
    top,
    offset: Math.max(8, Math.min(top, innerHeight - 202)) - top,
    flyoutLeft: left >= 210,
  };
}
function outsideMenu(event: Event) {
  const node = event.target;
  if (node instanceof Node && !popup.value?.contains(node) && !menuOpener.value?.contains(node))
    closeMenu();
}
function movedMenu() {
  const rect = menuOpener.value?.getBoundingClientRect();
  if (
    !rect ||
    !triggerPosition ||
    rect.top !== triggerPosition.top ||
    rect.left !== triggerPosition.left
  )
    closeMenu();
}
function hideMenu() {
  closeMenu();
}
function closeMenu(restore = false) {
  menu.value = reasons.value = false;
  if (restore) menuOpener.value?.focus({ preventScroll: true });
}
async function openMenu(event?: MouseEvent) {
  if (!mayDelete.value && !mayReport.value) return;
  if (event) {
    if (event.target instanceof HTMLElement && event.target.closest('[data-community-forwarded]'))
      return;
    event.preventDefault();
  }
  positionMenu(event);
  menu.value = true;
  await nextTick();
  primaryItem.value?.focus({ preventScroll: true });
}
function menuFocus(event: FocusEvent) {
  const next = event.relatedTarget;
  if (next instanceof Node && (popup.value?.contains(next) || menuOpener.value?.contains(next)))
    return;
  closeMenu();
}
watch(menu, (open, _, cleanup) => {
  if (!open) return;
  window.addEventListener('pointerdown', outsideMenu, true);
  window.addEventListener('scroll', movedMenu, true);
  window.addEventListener('resize', hideMenu);
  window.addEventListener('blur', hideMenu);
  cleanup(() => {
    window.removeEventListener('pointerdown', outsideMenu, true);
    window.removeEventListener('scroll', movedMenu, true);
    window.removeEventListener('resize', hideMenu);
    window.removeEventListener('blur', hideMenu);
  });
});
async function openReasons(focus = true) {
  if (!mayReport.value) return;
  reasons.value = true;
  if (focus) {
    await nextTick();
    reasonList.value?.querySelector('button')?.focus({ preventScroll: true });
  }
}
function menuKeys(event: KeyboardEvent) {
  const inReasons = reasonList.value?.contains(document.activeElement);
  if (event.key === 'Escape' || (event.key === 'ArrowLeft' && inReasons)) {
    event.preventDefault();
    event.stopPropagation();
    if (inReasons) {
      reasons.value = false;
      primaryItem.value?.focus({ preventScroll: true });
    } else closeMenu(true);
  } else if (event.key === 'ArrowRight' && document.activeElement === primaryItem.value) {
    event.preventDefault();
    void openReasons();
  } else if (inReasons && ['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
    event.preventDefault();
    const items = Array.from(reasonList.value?.querySelectorAll('button') ?? []);
    if (!items.length) return;
    const current = items.findIndex((node) => node === document.activeElement);
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? items.length - 1
          : (current + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
    items[next]?.focus({ preventScroll: true });
  }
}
function deleteNote() {
  if (!mayDelete.value) return;
  closeMenu();
  deletion?.open(props.card.key, menuOpener.value);
}
function reportNote(choice: ProfileReportChoice) {
  if (!mayReport.value) return;
  closeMenu(true);
  reporter?.open(props.card.key, props.card.authorName, choice, menuOpener.value);
}
watch(
  () => [props.card.key, mayDelete.value, mayReport.value],
  () => closeMenu(),
  { flush: 'sync' },
);
const date = new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium', timeStyle: 'short' });
const labels = {
  track: '单曲',
  playlist: '歌单',
  album: '专辑',
  artist: '歌手',
  podcast: '播客',
  voice: '声音',
};
</script>

<template>
  <article
    class="community-event-card"
    :data-event-key="nested ? undefined : card.key"
    :data-community-forwarded="nested ? '' : undefined"
    @contextmenu="openMenu"
  >
    <header class="community-author">
      <img
        v-if="card.avatarUrl"
        :src="card.avatarUrl"
        alt=""
        loading="lazy"
        referrerpolicy="no-referrer"
      />
      <span v-else class="community-avatar-fallback" aria-hidden="true"><Icon name="user" /></span>
      <div>
        <strong>{{ card.authorName }}</strong>
        <p class="muted">
          {{ card.timeText ?? (card.eventTime !== null ? date.format(card.eventTime) : '') }}
        </p>
      </div>
      <button
        v-if="card.detail"
        :data-community-opener="card.detail"
        class="text-button community-open"
        type="button"
        :aria-label="`查看 ${card.authorName} 的动态详情`"
        @click="emit('open', card.detail)"
      >
        查看详情
      </button>
      <div
        v-if="mayDelete || mayReport"
        class="community-note-menu"
        :class="{ 'menu-without-detail-link': !card.detail }"
        @focusout="menuFocus"
      >
        <button
          ref="menuOpener"
          class="icon-button"
          type="button"
          aria-label="动态操作"
          aria-haspopup="menu"
          :aria-expanded="menu"
          @click="menu ? closeMenu() : openMenu($event)"
          @keydown.down.prevent="openMenu()"
        >
          <Icon name="more" />
        </button>
        <Teleport to="body">
          <div
            v-if="menu"
            ref="popup"
            role="menu"
            class="community-note-menu-items"
            :data-event-menu-key="card.key"
            :style="anchorStyle"
            @keydown="menuKeys"
            @focusout="menuFocus"
          >
            <button
              v-if="mayDelete"
              ref="primaryItem"
              class="text-button"
              role="menuitem"
              type="button"
              @click="deleteNote"
            >
              删除
            </button>
            <template v-else-if="mayReport">
              <button
                ref="primaryItem"
                class="text-button"
                role="menuitem"
                type="button"
                aria-haspopup="menu"
                :aria-expanded="reasons"
                @click="openReasons()"
                @pointerenter="openReasons(false)"
              >
                举报 <span aria-hidden="true">›</span>
              </button>
              <div
                v-if="reasons"
                ref="reasonList"
                role="menu"
                aria-label="举报原因"
                data-community-report-reasons
                class="community-note-menu-items community-report-reasons"
                :class="{ 'community-report-reasons-left': anchor.flyoutLeft }"
                :style="reasonStyle"
              >
                <button
                  v-for="[choice, label] in profileReportChoices"
                  :key="choice"
                  class="text-button"
                  type="button"
                  role="menuitem"
                  @click="reportNote(choice)"
                >
                  {{ label }} <span v-if="choice === 'other'" aria-hidden="true">›</span>
                </button>
              </div>
            </template>
          </div>
        </Teleport>
      </div>
    </header>
    <p v-if="card.status === 'unsupported'" class="muted">
      该笔记类型暂不支持展示，请前往 App 查看。
    </p>
    <p v-else-if="card.status === 'unavailable'" class="muted">暂时无法显示这条动态。</p>
    <template v-else>
      <h2 v-if="card.title" class="community-event-title">
        <CommunityTopicText
          :value="card.title"
          :source="`${sourceKey ?? card.key}:title`"
          @topic="(name, opener) => emit('topic', name, opener)"
        />
      </h2>
      <p v-if="card.message" class="community-event-text">
        <CommunityTopicText
          :value="card.message"
          :source="`${sourceKey ?? card.key}:message`"
          @topic="(name, opener) => emit('topic', name, opener)"
        />
      </p>
      <div v-if="card.pictures.length" class="community-pictures">
        <img
          v-for="(url, index) in card.pictures"
          :key="`${index}:${url}`"
          :src="url"
          :alt="`动态配图 ${index + 1}`"
          loading="lazy"
          referrerpolicy="no-referrer"
        />
      </div>
      <p v-if="card.status === 'deleted'" class="muted">该动态已删除</p>
      <template v-else-if="card.attachment">
        <button
          v-if="card.attachment.type === 'resource'"
          type="button"
          class="community-resource"
          :disabled="['track', 'voice'].includes(card.attachment.kind) && !playable"
          :aria-label="`${['track', 'voice'].includes(card.attachment.kind) ? '播放' : '打开'} ${card.attachment.label}`"
          @click="emit('resource', card.attachment)"
        >
          <img
            v-if="card.attachment.coverUrl"
            :src="card.attachment.coverUrl"
            alt=""
            loading="lazy"
          />
          <Icon v-else name="music" />
          <span
            ><small class="muted">{{ labels[card.attachment.kind] }}</small
            >{{ card.attachment.label }}</span
          >
        </button>
        <CommunityEventCard
          v-else-if="card.attachment.type === 'forwarded' && !props.nested"
          :card="card.attachment.card"
          :source-key="`${sourceKey ?? card.key}:forward`"
          :playable="playable"
          nested
          @resource="(value) => emit('resource', value)"
          @topic="(name, opener) => emit('topic', name, opener)"
        />
        <p v-else-if="card.attachment.type === 'unavailable'" class="muted">
          {{ card.attachment.label }}
        </p>
      </template>
    </template>
    <footer v-if="!nested && card.status === 'ready'" class="community-counts muted">
      <span v-if="card.counters.comments !== null">评论 {{ card.counters.comments }}</span>
      <span v-if="card.counters.likes !== null">赞 {{ card.counters.likes }}</span>
      <span v-if="card.counters.shares !== null">分享 {{ card.counters.shares }}</span>
    </footer>
  </article>
</template>

<style scoped>
.community-note-menu {
  position: relative;
  margin-left: 6px;
}
.menu-without-detail-link {
  margin-left: auto;
}
.community-note-menu-items {
  position: fixed;
  z-index: 60;
  width: 112px;
  box-sizing: border-box;
  padding: 4px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface-raised);
}
.community-report-reasons {
  position: absolute;
  left: 100%;
  width: max-content;
  max-width: calc(100vw - 24px);
}
.community-report-reasons-left {
  left: auto;
  right: 100%;
}
.community-note-menu-items button {
  display: flex;
  width: 100%;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 6px 8px;
  color: var(--text);
  text-align: start;
}
.community-note-menu-items button:hover,
.community-note-menu-items button:focus-visible {
  background: var(--surface-alt);
}
</style>

<style scoped>
.community-event-card {
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 16px;
  display: grid;
  gap: 10px;
  overflow-wrap: anywhere;
}
.community-author {
  display: flex;
  align-items: center;
  gap: 10px;
}
.community-author img,
.community-avatar-fallback {
  width: 42px;
  height: 42px;
  border-radius: 50%;
  flex: none;
  object-fit: cover;
}
.community-avatar-fallback {
  display: grid;
  place-items: center;
  color: var(--text-secondary);
  background: var(--surface-alt);
}
.community-author p,
.community-event-card > p {
  margin: 0;
}
.community-author p {
  font-size: 12px;
  margin-block-start: 3px;
}
.community-open {
  margin-inline-start: auto;
  flex: none;
}
.community-event-title {
  margin: 0;
  font-size: 17px;
}
.community-event-text {
  white-space: pre-wrap;
  line-height: 1.55;
}
.community-pictures {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 108px));
  gap: 6px;
}
.community-pictures img {
  display: block;
  width: 100%;
  aspect-ratio: 1;
  object-fit: cover;
  border-radius: 7px;
}
.community-resource {
  display: inline-flex;
  justify-self: start;
  align-items: center;
  gap: 9px;
  min-width: 0;
  text-align: start;
  color: inherit;
  background: var(--surface-alt);
  border-radius: 8px;
  padding: 8px;
}
.community-resource:hover {
  background: var(--surface-hover);
}
.community-resource span {
  display: grid;
  gap: 3px;
}
.community-resource img {
  width: 36px;
  height: 36px;
  object-fit: cover;
  border-radius: 4px;
}
.community-counts {
  display: flex;
  flex-wrap: wrap;
  gap: 14px;
  font-size: 12px;
}
</style>
