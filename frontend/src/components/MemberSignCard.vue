<script setup lang="ts">
import { computed, watch, ref } from 'vue';
import type { useMemberSign } from '../composables/useMemberSign';
import Icon from './Icon.vue';

const props = defineProps<{ model: ReturnType<typeof useMemberSign>; playable: boolean }>();
const emit = defineEmits<{ play: [id: string] }>();
const card = computed(() => props.model.card.value);
const detail = computed(() => props.model.detail.value);
const selected = computed(() => props.model.day.value);
const today = computed(() => card.value?.days.find((day) => day.today));
const dialog = ref<HTMLDialogElement>();
watch(
  selected,
  (day) => {
    const node = dialog.value;
    if (day && node && !node.open) node.showModal();
    else if (!day && node?.open) node.close();
  },
  { flush: 'post' },
);
const date = computed(() =>
  detail.value
    ? new Intl.DateTimeFormat('zh-CN', { month: 'long', day: 'numeric' }).format(detail.value.time)
    : '',
);
</script>

<template>
  <section class="sign-card" aria-labelledby="member-sign-title" :aria-busy="model.cardBusy.value">
    <img
      v-if="card?.textureUrl"
      class="sign-texture"
      :src="card.textureUrl"
      alt=""
      loading="lazy"
      referrerpolicy="no-referrer"
    />
    <header>
      <h2 id="member-sign-title">{{ card?.title || '黑胶乐签' }}</h2>
      <p v-if="card?.subtitle">{{ card.subtitle }}</p>
    </header>
    <div v-if="model.cardError.value" class="sign-notice" role="status">
      {{ card ? '暂时无法更新，正在显示上次读取的记录' : '暂时无法读取乐签' }}
      <button class="sign-button" type="button" @click="model.load()">重试</button>
    </div>
    <p v-else-if="model.cardBusy.value && !card" class="sign-notice" role="status">正在读取乐签…</p>
    <template v-if="card">
      <ol v-if="card.days.length" class="sign-days" aria-label="最近打卡记录">
        <li v-for="(day, i) in card.days" :key="i" :data-state="day.signed ? 'signed' : 'unsigned'">
          <span>{{ day.today ? '今天' : day.label }}</span>
          <button
            v-if="day.signed && day.signTime"
            class="sign-stamp"
            type="button"
            :aria-label="`查看${day.today ? '今天' : day.label}乐签`"
            @click="model.open(day)"
          >
            <img v-if="day.coverUrl" :src="day.coverUrl" alt="" referrerpolicy="no-referrer" />
            <Icon v-else name="music" />
          </button>
          <span v-else class="sign-stamp" :aria-label="day.signed ? '已打卡' : '未打卡'">
            <Icon v-if="day.signed" name="music" />
            <span v-else aria-hidden="true">–</span>
          </span>
        </li>
      </ol>
      <p v-else class="sign-notice">暂无打卡记录</p>
      <button
        v-if="today?.signed && today.signTime"
        class="sign-button sign-today"
        type="button"
        @click="model.open(today)"
      >
        {{ card.buttonText || '查看乐签' }}
      </button>
      <span v-else-if="today" class="sign-notice">{{
        today.signed ? '今日已打卡' : '今日未打卡'
      }}</span>
    </template>
    <dialog
      ref="dialog"
      class="sign-dialog"
      aria-labelledby="member-sign-detail-title"
      @cancel.prevent="model.close()"
    >
      <header>
        <h2 id="member-sign-detail-title">
          {{ date || (selected?.today ? '今天' : selected?.label) }} · 黑胶乐签
        </h2>
        <button class="icon-button" aria-label="关闭乐签" autofocus @click="model.close()">
          <Icon name="close" />
        </button>
      </header>
      <p v-if="model.detailBusy.value" role="status">正在读取乐签详情…</p>
      <div v-else-if="model.detailError.value && selected" role="alert">
        {{ model.detailError.value }}
        <button class="sign-button" type="button" @click="model.open(selected)">重试</button>
      </div>
      <template v-else-if="detail">
        <div class="sign-detail-body">
          <div>
            <h3>{{ detail.song }}</h3>
            <p v-if="detail.artist">{{ detail.artist }}</p>
            <blockquote v-if="detail.wish">
              {{ detail.wish }}
              <cite>{{ detail.wishAuthor ? `来自${detail.wishAuthor}的评论` : '音乐寄语' }}</cite>
            </blockquote>
          </div>
          <div class="sign-detail-cover">
            <img
              v-if="detail.coverUrl"
              :src="detail.coverUrl"
              alt=""
              referrerpolicy="no-referrer"
            />
            <Icon v-else name="music" />
            <button
              class="icon-button"
              type="button"
              :disabled="!playable"
              :aria-label="`播放 ${detail.song}`"
              @click="emit('play', detail.songId)"
            >
              <Icon name="play" />
            </button>
          </div>
        </div>
        <footer v-if="detail.monthTotal !== null || detail.nextReward">
          <span v-if="detail.monthTotal !== null"
            >本月累计打卡 <strong>{{ detail.monthTotal }}</strong> 天</span
          >
          <span v-if="detail.nextReward"
            >再打卡 <strong>{{ detail.nextReward.daysToGo }}</strong> 天可得
            {{ detail.nextReward.subtitle }}{{ detail.nextReward.title }}</span
          >
        </footer>
      </template>
    </dialog>
  </section>
</template>

<style scoped>
.sign-card {
  position: relative;
  isolation: isolate;
  overflow: hidden;
  display: grid;
  justify-items: start;
  gap: 8px;
  padding: 14px 18px;
  border-radius: 10px;
  color: #312820;
  background: linear-gradient(320deg, #ffe0b5, #fff3e3);
}
.sign-texture {
  position: absolute;
  z-index: -1;
  inset: auto 0 0 auto;
  width: 57%;
  height: 70%;
  object-fit: fill;
  pointer-events: none;
}
.sign-card h2,
.sign-card p,
.sign-dialog h3 {
  margin: 0;
}
.sign-card h2 {
  font-size: 16px;
}
.sign-card p,
.sign-notice {
  font-size: 12px;
  color: #72563f;
}
.sign-card header p {
  margin-top: 4px;
}
.sign-days {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  margin: 0;
  padding: 0;
  list-style: none;
}
.sign-days li {
  display: grid;
  gap: 3px;
  justify-items: center;
  font-size: 11px;
}
.sign-stamp {
  display: grid;
  place-items: center;
  overflow: hidden;
  inline-size: 36px;
  block-size: 36px;
  border: 1px dashed #b9966e;
  border-radius: 7px;
  background: #fff8f0;
  color: #92622d;
  padding: 0;
}
[data-state='signed'] .sign-stamp {
  border-style: solid;
}
button.sign-stamp,
.sign-button {
  cursor: pointer;
}
button.sign-stamp:hover,
.sign-button:hover {
  background: #ffe8ca;
}
.sign-stamp img,
.sign-detail-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.sign-button {
  border: 1px solid currentColor;
  border-radius: 999px;
  padding: 3px 9px;
  background: transparent;
  color: #915214;
  font: inherit;
  font-size: 12px;
}
.sign-dialog {
  box-sizing: border-box;
  width: min(574px, calc(100vw - 32px));
  max-height: calc(100vh - 32px);
  overflow: auto;
  padding: 22px clamp(14px, 4vw, 30px);
  border: 0;
  border-radius: 12px;
  color: #172031;
  background: linear-gradient(180deg, #c1d4ff, #fff 74%);
  box-shadow: 0 18px 48px #0004;
}
.sign-dialog::backdrop {
  background: #1119;
}
.sign-dialog header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  margin-bottom: 16px;
}
.sign-dialog header .icon-button {
  color: inherit;
}
.sign-detail-body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(100px, 45%);
  gap: 18px;
}
.sign-detail-body h3 {
  font-size: clamp(18px, 3vw, 26px);
  overflow-wrap: anywhere;
}
.sign-detail-body p {
  color: #465273;
  margin: 6px 0;
}
.sign-detail-body blockquote {
  border-top: 1px solid #31436133;
  margin: 14px 0 0;
  padding-top: 12px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}
.sign-detail-body cite {
  display: block;
  text-align: right;
  color: #465273;
  font-size: 12px;
  font-style: normal;
}
.sign-detail-cover {
  position: relative;
  display: grid;
  place-items: center;
  aspect-ratio: 1;
  overflow: hidden;
  border-radius: 8px;
  background: #748eb955;
}
.sign-detail-cover > .icon {
  width: 34%;
  height: 34%;
}
.sign-detail-cover button {
  position: absolute;
  right: 10px;
  bottom: 10px;
  color: white;
  background: #172031d9;
  border-radius: 50%;
}
.sign-dialog footer {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 18px;
  margin-top: 16px;
  padding-top: 14px;
  border-top: 1px dashed #65708a88;
  font-size: 13px;
}
</style>
