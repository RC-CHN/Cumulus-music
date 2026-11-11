<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { togetherRecordQrPath, togetherRecordQrSize } from '../data/together-record-qr';
import { togetherRecordDuration, type TogetherLeaveRecord } from '../services/together';
import Icon from './Icon.vue';

const props = defineProps<{ record: TogetherLeaveRecord }>();
const emit = defineEmits<{ close: [operationId: string] }>();
const dialog = ref<HTMLDialogElement>();
const failedAvatars = ref(new Set<string>());
const avatars = computed(() => [
  { label: '我的头像', url: props.record.selfAvatarUrl },
  { label: '听友头像', url: props.record.peerAvatarUrl },
]);
const stats = computed(() => [
  { label: '本次一起听了', value: `${props.record.thisListenCount}首歌曲` },
  { label: '本次陪伴彼此', value: togetherRecordDuration(props.record.thisDuration) },
]);
const total = computed(
  () =>
    `累计${props.record.totalListenCount}首歌曲，${togetherRecordDuration(props.record.totalDuration)}`,
);
const qrBox = `0 0 ${togetherRecordQrSize} ${togetherRecordQrSize}`;
function close() {
  emit('close', props.record.operationId);
}
onMounted(() => dialog.value?.showModal());
</script>
<template>
  <dialog
    ref="dialog"
    class="together-leave-record"
    data-together-leave-record
    aria-label="一起听记录"
    @cancel.prevent="close"
  >
    <article class="record-content">
      <button
        type="button"
        class="icon-button record-close"
        aria-label="关闭一起听记录"
        autofocus
        @click="close"
      >
        <Icon name="close" />
      </button>
      <div class="record-avatars">
        <span v-for="avatar in avatars" :key="avatar.label" role="img" :aria-label="avatar.label">
          <img
            v-if="avatar.url && !failedAvatars.has(avatar.url)"
            :src="avatar.url"
            alt=""
            referrerpolicy="no-referrer"
            @error="failedAvatars.add(avatar.url)"
          />
          <Icon v-else name="user" />
        </span>
      </div>
      <dl class="record-stats">
        <div v-for="stat in stats" :key="stat.label">
          <dt>{{ stat.label }}</dt>
          <dd>
            <template v-for="(part, index) in stat.value.match(/\d+|\D+/g)" :key="index">
              <strong v-if="/^\d+$/.test(part)">{{ part }}</strong>
              <span v-else>{{ part }}</span>
            </template>
          </dd>
        </div>
      </dl>
      <p class="record-total">{{ total }}</p>
      <footer class="record-footer">
        <div>
          <strong>Cumulus Music</strong>
          <p>识别二维码获得更多精彩内容</p>
        </div>
        <svg
          class="record-qr"
          data-together-record-qr
          role="img"
          aria-label="网易云音乐公开活动二维码"
          :viewBox="qrBox"
          width="72"
          height="72"
          shape-rendering="crispEdges"
        >
          <rect width="100%" height="100%" fill="#fff" />
          <path :d="togetherRecordQrPath" fill="#000" />
        </svg>
      </footer>
    </article>
  </dialog>
</template>
<style scoped>
.together-leave-record {
  width: 442px;
  height: 404px;
  max-width: calc(100vw - 24px);
  max-height: calc(100dvh - 24px);
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 14px;
  color: var(--text);
  background: var(--surface);
  box-shadow: var(--shadow);
  overflow: auto;
}
.together-leave-record::backdrop {
  background: #10111980;
}
.record-content {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  min-height: 100%;
  padding-top: 40px;
}
.record-close {
  position: absolute;
  top: 12px;
  inset-inline-end: 12px;
}
.record-avatars {
  display: flex;
}
.record-avatars > span {
  display: grid;
  place-items: center;
  width: 88px;
  height: 88px;
  margin: 0 -4px;
  border: 3px solid var(--surface);
  border-radius: 50%;
  color: var(--accent);
  background: var(--surface-hover);
  overflow: hidden;
}
.record-avatars img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.record-avatars .icon {
  width: 35px;
  height: 35px;
}
.record-stats {
  display: grid;
  grid-template-columns: minmax(0, max-content) minmax(0, max-content);
  justify-content: center;
  gap: min(80px, 16vw);
  width: 100%;
  margin: 24px 0 0;
  padding: 0 16px 11px;
  text-align: center;
  color: var(--muted);
}
.record-stats dt {
  padding: 1px 0;
  font-size: 16px;
}
.record-stats dd {
  min-height: 46px;
  margin: 0;
  font-size: 14px;
  overflow-wrap: anywhere;
}
.record-stats dd strong {
  margin: 0 4px;
  color: var(--text);
  font-size: 32px;
  font-weight: 500;
}
.record-total {
  align-self: stretch;
  margin: 0 min(80px, 15vw);
  padding: 20px 0;
  border-top: 1px solid var(--line);
  color: var(--muted);
  font-size: 16px;
  text-align: center;
  overflow-wrap: anywhere;
}
.record-footer {
  display: flex;
  flex: none;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  width: 100%;
  min-height: 108px;
  margin-top: auto;
  padding: 12px min(30px, 6vw) 12px min(40px, 8vw);
  background: var(--surface-alt);
}
.record-footer strong {
  font-size: 20px;
  line-height: 1;
}
.record-footer p {
  margin: 7px 0 0;
  color: var(--muted);
  font-size: 12px;
}
.record-qr {
  flex: none;
  box-sizing: border-box;
  border: 1px solid #000;
  background: #fff;
}
</style>
