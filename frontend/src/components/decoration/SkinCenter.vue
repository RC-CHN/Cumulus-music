<script setup lang="ts">
import { nextTick, onMounted, ref } from 'vue';
import { useAppearance } from '../../composables/useAppearance';
import Icon from '../Icon.vue';
import CustomSkinDialog from './CustomSkinDialog.vue';
import RemoteSkinCatalogue from './RemoteSkinCatalogue.vue';
import type { AccountView } from '../../services/contracts';
defineProps<{ account: AccountView }>();

const { preference, select, customColor, saved } = useAppearance();
const title = ref<HTMLElement>();
const customCard = ref<HTMLButtonElement>();
const customOpen = ref(false);
const cards = [
  { id: 'light', label: '默认' },
  { id: 'red', label: '经典红' },
  { id: 'dark', label: '酷炫黑' },
] as const;
onMounted(() => title.value?.focus({ preventScroll: true }));
async function closeCustom() {
  customOpen.value = false;
  await nextTick();
  customCard.value?.focus({ preventScroll: true });
}
</script>

<template>
  <section class="skin-center" aria-labelledby="skin-center-title">
    <h1 id="skin-center-title" ref="title" tabindex="-1">个性皮肤</h1>
    <RemoteSkinCatalogue :account="account">
      <h2 data-skin-section="official" tabindex="-1">官方</h2>
      <div class="skin-cards">
        <button
          v-for="card in cards"
          :key="card.id"
          type="button"
          class="skin-card"
          :data-skin="card.id"
          :aria-pressed="preference === card.id"
          @click="select(card.id)"
        >
          <span class="skin-art" aria-hidden="true">
            <span class="skin-mini">
              <span class="skin-mini-nav"><i /><i /><i /></span>
              <span class="skin-mini-body"><i /><i /><i /><i /></span>
              <span class="skin-mini-player"><Icon name="play" /></span>
            </span>
          </span>
          <span class="skin-card-name">{{ card.label }}</span>
          <span v-if="preference === card.id" class="skin-selected" aria-label="使用中">
            <Icon name="check" />
          </span>
        </button>
        <button
          ref="customCard"
          type="button"
          class="skin-card"
          data-skin="custom"
          aria-haspopup="dialog"
          :aria-pressed="preference === 'custom'"
          :style="customColor ? { '--skin-choice': customColor } : undefined"
          @click="customOpen = true"
        >
          <span class="skin-art skin-color-art" aria-hidden="true">
            <Icon name="palette" />
          </span>
          <span class="skin-card-name">按钮颜色自定义</span>
          <span v-if="preference === 'custom'" class="skin-selected" aria-label="使用中">
            <Icon name="check" />
          </span>
        </button>
      </div>
    </RemoteSkinCatalogue>
    <p v-if="!saved" class="catalog-error" role="status">主题已应用，但暂时无法保存到本机。</p>
    <CustomSkinDialog v-if="customOpen" @close="closeCustom" />
  </section>
</template>

<style scoped>
.skin-center {
  width: 100%;
  min-height: 0;
  overflow: auto;
  padding: 28px clamp(16px, 4vw, 48px) 38px;
}
.skin-center h1 {
  margin: 0;
  padding-bottom: 14px;
  border-bottom: 1px solid var(--line);
  font-size: 24px;
  outline: none;
}
.skin-center h2 {
  margin: 25px 0 14px;
  font-size: 19px;
}
.skin-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 162px), 1fr));
  gap: 18px;
  max-width: 960px;
}
.skin-card {
  position: relative;
  display: grid;
  min-width: 0;
  gap: 10px;
  padding: 0 0 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--surface-raised);
  color: var(--text);
  text-align: left;
}
.skin-card:hover {
  border-color: var(--line-strong);
  background: var(--surface-alt);
}
.skin-card[aria-pressed='true'] {
  border-color: var(--accent);
}
.skin-card:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 3px;
}
.skin-card-name {
  padding-inline: 12px;
  font-size: 14px;
  font-weight: 560;
}
.skin-art {
  display: grid;
  place-items: center;
  position: relative;
  aspect-ratio: 16/10;
  overflow: hidden;
  border-radius: 9px 9px 0 0;
  background: #eeeaf4;
  --mini-surface: #fff;
  --mini-side: #f2eff6;
  --mini-ink: #e4dbe9;
  --mini-accent: #cf2d51;
}
.skin-card[data-skin='red'] .skin-art {
  background: #f7d9dc;
  --mini-side: #ffeeef;
  --mini-ink: #ead9dd;
  --mini-accent: #c32731;
}
.skin-card[data-skin='dark'] .skin-art {
  background: #383645;
  --mini-surface: #22232d;
  --mini-side: #191923;
  --mini-ink: #484354;
  --mini-accent: #f77c99;
}
.skin-mini {
  display: grid;
  width: 77%;
  height: 72%;
  grid-template-columns: 29% minmax(0, 1fr);
  grid-template-rows: minmax(0, 1fr) 24%;
  overflow: hidden;
  border-radius: 5px;
  background: var(--mini-surface);
  box-shadow: 0 3px 13px #23152733;
}
.skin-mini-nav,
.skin-mini-body {
  display: flex;
  flex-direction: column;
  gap: 8%;
  justify-content: center;
  padding: 11% 12%;
}
.skin-mini-nav {
  background: var(--mini-side);
}
.skin-mini :is(.skin-mini-nav, .skin-mini-body) i {
  height: 10%;
  border-radius: 4px;
  background: var(--mini-ink);
}
.skin-mini-nav i:first-child,
.skin-mini-body i:first-child {
  background: var(--mini-accent);
}
.skin-mini-body i:first-child {
  width: 43%;
}
.skin-mini-body i:nth-child(3) {
  width: 76%;
}
.skin-mini-player {
  grid-column: 1/-1;
  display: grid;
  place-items: center;
  border-top: 1px solid var(--mini-ink);
  color: var(--mini-accent);
}
.skin-mini-player .icon {
  width: 12px;
  height: 12px;
}
.skin-color-art {
  color: var(--skin-choice, #a33869);
  background: radial-gradient(circle at 72% 25%, #ffecd1, #f2d9e9 44%, #d8d8fa);
}
.skin-color-art .icon {
  width: 36%;
  height: 57%;
}
.skin-selected {
  position: absolute;
  top: 8px;
  right: 8px;
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border-radius: 50%;
  color: var(--on-accent);
  background: var(--accent-fill);
}
.skin-selected .icon {
  width: 15px;
  height: 15px;
}
</style>
