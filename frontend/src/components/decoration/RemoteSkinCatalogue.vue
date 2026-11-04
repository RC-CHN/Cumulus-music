<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, shallowRef, useId, watch } from 'vue';
import { useCore } from '../../services/core';
import type { AccountView } from '../../services/contracts';
import type { RemoteSkinCard, RemoteSkinCatalog } from '../../services/remoteSkins';
import Icon from '../Icon.vue';

const props = defineProps<{ account: AccountView }>();
const core = useCore();
const region = ref<HTMLElement>();
const dialog = ref<HTMLDialogElement>();
const groupNav = ref<HTMLElement>();
const activeGroup = ref('official');
const catalogue = shallowRef<RemoteSkinCatalog>();
const state = ref<'guest' | 'loading' | 'failed' | 'ready'>('guest');
const covers = ref<Record<number, string>>({});
const preview = ref<{
  card: RemoteSkinCard;
  status: 'loading' | 'failed' | 'ready';
  src?: string;
}>();
const previewTitle = useId();
const corners = { popular: '热门', new: 'NEW', limited: '限量' };
const entitlements = { vinyl: '黑胶 VIP', super_vinyl: '黑胶 SVIP' };
let origin: HTMLButtonElement | undefined;
let catalogTurn = 0;
let previewTurn = 0;
let mounted = true;
let observer: IntersectionObserver | undefined;
let coverActive = 0;
const pending: { key: number; epoch: string; revision: string }[] = [];

function png(value: string) {
  if (value.length <= 4 * 1024 * 1024 + 32 && value.startsWith('data:image/png;base64,'))
    return value;
  throw new Error('Invalid skin image');
}
function pumpCovers() {
  while (mounted && coverActive < 2 && pending.length) {
    const item = pending.shift()!;
    if (catalogue.value?.revision !== item.revision || props.account.epoch !== item.epoch) continue;
    coverActive++;
    void core
      .remoteSkinImage(item.epoch, item.revision, item.key, 'cover')
      .then((reply) => {
        if (
          mounted &&
          props.account.epoch === reply.epoch &&
          catalogue.value?.revision === item.revision
        )
          covers.value[item.key] = png(reply.value);
      })
      .catch(() => {}) // A thumbnail failure leaves its own neutral artwork.
      .finally(() => {
        coverActive--;
        pumpCovers();
      });
  }
}
function observeCovers(epoch: string, revision: string) {
  const nodes = region.value?.querySelectorAll<HTMLElement>('[data-remote-skin-cover]');
  if (!nodes || !nodes.length) return;
  const queue = (node: HTMLElement) => {
    const key = Number(node.dataset.remoteSkinCover);
    pending.push({ epoch, revision, key });
  };
  if (typeof IntersectionObserver !== 'undefined') {
    observer = new IntersectionObserver(
      (entries) => {
        if (!mounted || catalogue.value?.revision !== revision || props.account.epoch !== epoch)
          return;
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          observer?.unobserve(entry.target);
          queue(entry.target as HTMLElement);
        }
        pumpCovers();
      },
      { root: region.value?.closest('.skin-center') ?? null, rootMargin: '96px' },
    );
    nodes.forEach((node) => observer?.observe(node));
  } else {
    [...nodes].slice(0, 8).forEach(queue);
    pumpCovers();
  }
}
async function closePreview(restore = true) {
  const target = origin;
  origin = undefined;
  previewTurn++;
  dialog.value?.close();
  preview.value = undefined;
  if (restore) {
    await nextTick();
    if (target?.isConnected) target.focus({ preventScroll: true });
  }
}
async function load() {
  const turn = ++catalogTurn;
  observer?.disconnect();
  pending.length = 0;
  covers.value = {};
  catalogue.value = undefined;
  activeGroup.value = 'official';
  void closePreview(false);
  const { epoch, status } = props.account;
  if (status !== 'authenticated') {
    state.value = 'guest';
    return;
  }
  state.value = 'loading';
  try {
    const reply = await core.remoteSkinCatalog(epoch);
    if (!mounted || turn !== catalogTurn || epoch !== props.account.epoch || reply.epoch !== epoch)
      return;
    catalogue.value = reply.value;
    state.value = 'ready';
    await nextTick();
    if (mounted && turn === catalogTurn) observeCovers(epoch, reply.value.revision);
  } catch {
    if (mounted && turn === catalogTurn && props.account.epoch === epoch) state.value = 'failed';
  }
}
async function requestPreview(card: RemoteSkinCard) {
  const revision = catalogue.value?.revision;
  if (!revision || !card.hasPreview || props.account.status !== 'authenticated') return;
  const epoch = props.account.epoch;
  const turn = ++previewTurn;
  preview.value = { card, status: 'loading' };
  await nextTick();
  if (!mounted || turn !== previewTurn || !dialog.value) return;
  if (!dialog.value.open) dialog.value.showModal();
  if (!dialog.value.contains(document.activeElement))
    dialog.value.querySelector<HTMLButtonElement>('header button')?.focus({ preventScroll: true });
  try {
    const reply = await core.remoteSkinImage(epoch, revision, card.key, 'preview');
    if (mounted && turn === previewTurn && props.account.epoch === epoch && reply.epoch === epoch)
      preview.value = { card, status: 'ready', src: png(reply.value) };
  } catch {
    if (mounted && turn === previewTurn && props.account.epoch === epoch)
      preview.value = { card, status: 'failed' };
  }
}
function openPreview(card: RemoteSkinCard, event: MouseEvent) {
  origin = event.currentTarget as HTMLButtonElement;
  void requestPreview(card);
}
function goTo(key: string) {
  const viewport = region.value?.closest('.skin-center');
  const target = region.value?.querySelector<HTMLElement>(`[data-skin-section="${key}"]`);
  if (!viewport || !target) return;
  activeGroup.value = key;
  const offset = (groupNav.value?.offsetHeight ?? 0) + 14;
  viewport.scrollTop +=
    target.getBoundingClientRect().top - viewport.getBoundingClientRect().top - offset;
  target.focus({ preventScroll: true });
}
watch(() => [props.account.epoch, props.account.status], load, { immediate: true });
onBeforeUnmount(() => {
  mounted = false;
  catalogTurn++;
  observer?.disconnect();
  pending.length = 0;
  void closePreview(false);
});
</script>

<template>
  <div ref="region">
    <nav
      v-if="catalogue?.groups.length"
      ref="groupNav"
      class="skin-group-nav"
      aria-label="皮肤分组"
    >
      <button
        type="button"
        :aria-current="activeGroup === 'official' ? 'location' : undefined"
        @click="goTo('official')"
      >
        官方
      </button>
      <button
        v-for="(group, index) in catalogue.groups"
        :key="index"
        type="button"
        :aria-current="activeGroup === String(index) ? 'location' : undefined"
        @click="goTo(String(index))"
      >
        {{ group.name }}
      </button>
    </nav>
    <slot />
    <p v-if="state === 'guest'" class="skin-read-state">登录后可查看在线皮肤。</p>
    <p v-else-if="state === 'loading'" class="skin-read-state" role="status">正在加载在线皮肤…</p>
    <p v-else-if="state === 'failed'" class="skin-read-state" role="status">
      在线皮肤加载失败。
      <button type="button" class="text-button" @click="load()">重试</button>
    </p>
    <p v-else-if="!catalogue?.groups.length" class="skin-read-state" role="status">
      暂无在线皮肤。
    </p>
    <section
      v-for="(group, index) in catalogue?.groups ?? []"
      :key="index"
      class="remote-skin-group"
    >
      <h2 :data-skin-section="String(index)" tabindex="-1">{{ group.name }}</h2>
      <div class="remote-skin-grid">
        <article v-for="card in group.cards" :key="card.key" class="remote-skin-card">
          <div
            class="remote-skin-art"
            :data-remote-skin-cover="card.hasCover ? card.key : undefined"
            aria-hidden="true"
          >
            <img v-if="covers[card.key]" :src="covers[card.key]" alt="" />
            <Icon v-else name="palette" />
            <span v-if="card.corner" class="skin-corner">{{ corners[card.corner] }}</span>
          </div>
          <div class="remote-skin-info">
            <p class="remote-skin-name">{{ card.name }}</p>
            <small v-if="card.entitlement">{{ entitlements[card.entitlement] }}</small>
            <button
              v-if="card.hasPreview"
              type="button"
              class="text-button"
              aria-haspopup="dialog"
              :aria-label="`预览 ${card.name}`"
              @click="openPreview(card, $event)"
            >
              预览
            </button>
          </div>
        </article>
      </div>
    </section>
    <dialog
      v-if="preview"
      ref="dialog"
      class="skin-preview"
      :aria-labelledby="previewTitle"
      @cancel.prevent="closePreview()"
    >
      <header>
        <h2 :id="previewTitle">皮肤预览</h2>
        <button type="button" class="icon-button" aria-label="关闭皮肤预览" @click="closePreview()">
          <Icon name="close" />
        </button>
      </header>
      <div class="skin-preview-art">
        <img v-if="preview.src" :src="preview.src" :alt="`${preview.card.name} 的皮肤预览`" />
        <p v-else-if="preview.status === 'loading'" role="status">正在加载预览…</p>
        <p v-else role="status">
          预览图片加载失败。
          <button type="button" class="text-button" @click="requestPreview(preview.card)">
            重试
          </button>
        </p>
      </div>
      <p class="skin-preview-caption">
        <strong>{{ preview.card.name }}</strong>
        <small v-if="preview.card.entitlement">{{ entitlements[preview.card.entitlement] }}</small>
      </p>
    </dialog>
  </div>
</template>

<style scoped>
.skin-group-nav {
  position: sticky;
  z-index: 1;
  top: 0;
  display: flex;
  gap: 8px;
  background: var(--surface);
  overflow-x: auto;
  margin-top: 15px;
  padding-block: 4px;
}
.skin-group-nav button {
  flex: 0 0 auto;
  padding: 4px 10px;
  border-radius: 14px;
  color: var(--text-secondary);
}
.skin-group-nav button[aria-current] {
  color: var(--accent);
  background: var(--accent-soft);
}
.skin-group-nav button:hover,
.remote-skin-info small,
.skin-preview-caption small {
  color: var(--accent);
}
.skin-read-state {
  margin: 20px 0 0;
  color: var(--text-secondary);
}
.remote-skin-group h2 {
  margin: 25px 0 14px;
  font-size: 19px;
}
.remote-skin-group h2:focus {
  outline: none;
  text-decoration: underline 2px var(--accent);
  text-underline-offset: 0.15em;
}
.remote-skin-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(100%, 162px), 1fr));
  gap: 18px;
  max-width: 960px;
}
.remote-skin-card {
  min-width: 0;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--surface-raised);
  overflow: hidden;
}
.remote-skin-art {
  position: relative;
  display: grid;
  place-items: center;
  aspect-ratio: 16/10;
  background: var(--art-bg);
  color: var(--art-color);
  overflow: hidden;
}
.remote-skin-art img {
  width: 100%;
  height: 100%;
  position: absolute;
  inset: 0;
  object-fit: cover;
}
.remote-skin-art .icon {
  width: 32px;
  height: 32px;
}
.skin-corner {
  position: absolute;
  top: 8px;
  right: 8px;
  padding: 3px 7px;
  border-radius: 6px;
  background: var(--surface-raised);
  color: var(--accent);
  font-size: 11px;
}
.remote-skin-info {
  padding: 10px 12px;
  display: flex;
  flex-wrap: wrap;
  gap: 5px 8px;
  align-items: center;
}
.remote-skin-name {
  margin: 0;
  flex: 1 0 100%;
  overflow-wrap: anywhere;
}
.remote-skin-info button {
  margin-left: auto;
}
.skin-preview {
  width: min(720px, calc(100vw - 32px));
  max-height: calc(100vh - 32px);
  overflow: auto;
  margin: auto;
  padding: 16px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--surface-raised);
  color: var(--text);
  box-shadow: var(--shadow);
}
.skin-preview::backdrop {
  background: var(--overlay);
}
.skin-preview header,
.skin-preview-caption {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}
.skin-preview header h2 {
  margin: 0;
  font-size: 19px;
}
.skin-preview-art {
  margin-top: 10px;
  min-height: min(160px, 27vh);
  display: grid;
  place-items: center;
  border-radius: 8px;
  background: var(--surface-alt);
}
.skin-preview-art img {
  display: block;
  width: 100%;
  max-height: min(65vh, 720px);
  object-fit: contain;
}
.skin-preview-caption {
  margin: 12px 0 0;
}
</style>
