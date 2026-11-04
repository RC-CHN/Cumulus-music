<script setup lang="ts">
import { computed, watch, ref } from 'vue';
import type { useMemberSign } from '../composables/useMemberSign';
import { validMemberQr, type MemberSignDay } from '../services/member';
import type { TrackCurrentToggle } from '../services/trackMenu';
import Icon from './Icon.vue';
import MemberQrImage from './MemberQrImage.vue';

const props = defineProps<{
  model: ReturnType<typeof useMemberSign>;
  playable: boolean;
  signedIn: boolean;
  liked: Set<string>;
  busyLikes: Set<string>;
  likeNotice: string;
  currentToggle?: TrackCurrentToggle;
  experience: 'VIP' | 'SVIP';
}>();
const emit = defineEmits<{
  play: [id: string];
  like: [id: string];
  toggle: [target: TrackCurrentToggle, anchor: HTMLButtonElement];
}>();
const card = computed(() => props.model.card.value);
const detail = computed(() => props.model.detail.value);
const selected = computed(() => props.model.day.value);
const activeSong = computed(() =>
  props.currentToggle?.trackId === detail.value?.songId ? props.currentToggle : undefined,
);
const likedSong = computed(() => !!detail.value && props.liked.has(detail.value.songId));
const likeDisabled = computed(
  () => !props.signedIn || !detail.value || props.busyLikes.has(detail.value.songId),
);
const likeRequest = ref<string>();
watch(detail, () => (likeRequest.value = undefined));
const portalMatrix = computed(() => {
  const modules = detail.value?.portalQr?.modules;
  return validMemberQr(modules) ? modules : undefined;
});
const rewardSubtitle = computed(() => {
  const value = detail.value?.nextReward?.subtitle ?? '';
  const split = value.match(/^(\d+)(.*)$/);
  return split ? [split[1], split[2]] : [value, ''];
});
function toggleSong(event: MouseEvent) {
  if (!props.playable || !detail.value) return;
  if (activeSong.value) emit('toggle', activeSong.value, event.currentTarget as HTMLButtonElement);
  else emit('play', detail.value.songId);
}
function likeSong() {
  if (!detail.value || likeDisabled.value) return;
  likeRequest.value = detail.value.songId;
  emit('like', detail.value.songId);
}
const today = computed(() => card.value?.days.find((day) => day.today));
const action = computed(() => props.model.action.value);
const canViewToday = computed(
  () => !!props.model.confirmedDetail.value || !!(today.value?.signed && today.value.signTime),
);
const buttonKind = computed(() => {
  if (action.value?.phase === 'unknown') return 'reconcile';
  if (canViewToday.value) return 'view';
  if (action.value?.phase === 'ready' && action.value.dayKey) return 'check-in';
});
const actionDisabled = computed(
  () =>
    props.model.signBusy.value ||
    props.model.cardBusy.value ||
    (buttonKind.value === 'check-in' && !!props.model.cardError.value),
);
const buttonText = computed(() => {
  if (props.model.signBusy.value)
    return buttonKind.value === 'reconcile' ? '正在核对…' : '正在打卡…';
  if (buttonKind.value === 'reconcile') return '核对打卡结果';
  if (buttonKind.value === 'view')
    return (today.value?.signed && card.value?.buttonText) || '查看乐签';
  return card.value?.buttonText || '立即打卡';
});
function isSigned(day: MemberSignDay) {
  return day.signed || (day.today && action.value?.phase === 'signed');
}
function viewDay(day: MemberSignDay) {
  if (props.model.signBusy.value) return;
  if (day.today && props.model.confirmedDetail.value) props.model.showConfirmedDetail();
  else void props.model.open(day);
}
function activateToday(kind: typeof buttonKind.value, dayKey?: string | null) {
  if (actionDisabled.value || kind !== buttonKind.value) return;
  if (kind === 'reconcile') void props.model.submit({ kind: 'reconcile' });
  else if (kind === 'check-in' && dayKey) void props.model.submit({ kind: 'check_in', dayKey });
  else if (kind === 'view' && props.model.confirmedDetail.value) props.model.showConfirmedDetail();
  else if (kind === 'view' && today.value) viewDay(today.value);
}
function activateRendered(event: MouseEvent) {
  const button = event.currentTarget as HTMLButtonElement;
  const kind = button.dataset.action;
  if (kind === 'view' || kind === 'reconcile' || kind === 'check-in')
    activateToday(kind, button.dataset.dayKey);
}
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
  <section
    class="sign-card"
    aria-labelledby="member-sign-title"
    :aria-busy="model.cardBusy.value || model.signBusy.value"
  >
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
        <li
          v-for="(day, i) in card.days"
          :key="i"
          :data-state="isSigned(day) ? 'signed' : 'unsigned'"
        >
          <span>{{ day.today ? '今天' : day.label }}</span>
          <button
            v-if="(day.signed && day.signTime) || (day.today && model.confirmedDetail.value)"
            class="sign-stamp"
            type="button"
            :aria-label="`查看${day.today ? '今天' : day.label}乐签`"
            :aria-disabled="model.signBusy.value"
            @click="viewDay(day)"
          >
            <img v-if="day.coverUrl" :src="day.coverUrl" alt="" referrerpolicy="no-referrer" />
            <Icon v-else name="music" />
          </button>
          <span v-else class="sign-stamp" :aria-label="isSigned(day) ? '已打卡' : '未打卡'">
            <Icon v-if="isSigned(day)" name="music" />
            <span v-else aria-hidden="true">–</span>
          </span>
        </li>
      </ol>
      <p v-else class="sign-notice">暂无打卡记录</p>
    </template>
    <p v-if="action?.phase === 'unknown'" class="sign-notice sign-action-notice" role="status">
      打卡结果待确认。核对只会读取记录。
    </p>
    <p v-if="model.signError.value" class="sign-notice sign-error" role="alert">
      {{ model.signError.value }}
    </p>
    <button
      v-if="buttonKind"
      class="sign-button"
      :class="{ 'sign-today': buttonKind === 'view' }"
      type="button"
      :aria-disabled="actionDisabled"
      :data-action="buttonKind"
      :data-day-key="action?.dayKey"
      @click="activateRendered"
    >
      {{ buttonText }}
    </button>
    <span v-else-if="today" class="sign-notice">{{
      isSigned(today) ? '今日已打卡' : '今日未打卡'
    }}</span>
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
            <div class="sign-detail-controls">
              <button
                class="icon-button"
                type="button"
                :aria-disabled="likeDisabled"
                :aria-pressed="likedSong"
                :aria-label="`${likedSong ? '取消喜欢' : '喜欢'} ${detail.song}`"
                @click="likeSong"
              >
                <Icon name="heart" :class="{ 'sign-liked': likedSong }" />
              </button>
              <button
                class="icon-button"
                type="button"
                :aria-disabled="!playable"
                :aria-label="`${activeSong?.pause ? '暂停' : activeSong ? '继续播放' : '播放'} ${detail.song}`"
                @click="toggleSong"
              >
                <Icon :name="activeSong?.pause ? 'pause' : 'play'" />
              </button>
            </div>
          </div>
        </div>
        <p
          v-if="likeRequest === detail.songId && likeNotice"
          class="sign-detail-notice"
          role="status"
        >
          {{ likeNotice }}
        </p>
        <footer>
          <span v-if="detail.monthTotal !== null"
            >本月累计打卡 <strong>{{ detail.monthTotal }}</strong> 天</span
          >
          <span v-if="detail.allRewardsClaimed"
            >每月打卡得 <strong>{{ experience }}</strong> 体验</span
          >
          <span v-else-if="detail.nextReward"
            >再打卡 <strong>{{ detail.nextReward.daysToGo }}</strong> 天可得
            <strong v-if="rewardSubtitle[0]">{{ rewardSubtitle[0] }}</strong
            >{{ rewardSubtitle[1] }}{{ detail.nextReward.title }}</span
          >
          <div v-if="portalMatrix" class="sign-detail-portal">
            <div>云音乐 App 扫码去会员中心<br /><strong>会员每日打卡得成长值</strong></div>
            <MemberQrImage :modules="portalMatrix" :width="60" label="乐签会员中心二维码" />
          </div>
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
button.sign-stamp[aria-disabled='true'],
.sign-button[aria-disabled='true'] {
  opacity: 0.65;
  cursor: default;
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
.sign-detail-controls {
  position: absolute;
  right: 10px;
  bottom: 10px;
  display: flex;
  gap: 6px;
}
.sign-detail-controls button {
  color: white;
  background: #172031d9;
  border-radius: 50%;
}
.sign-detail-controls button[aria-disabled='true'] {
  opacity: 0.65;
}
.sign-liked {
  fill: #ff4969;
  color: #ff4969;
}
.sign-detail-notice {
  margin: 8px 0 0;
  font-size: 13px;
}
.sign-dialog footer {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px 18px;
  margin-top: 16px;
  padding-top: 14px;
  border-top: 1px dashed #65708a88;
  font-size: 13px;
}
.sign-detail-portal {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
}
.sign-detail-portal strong {
  color: #8d2f3e;
}
</style>
