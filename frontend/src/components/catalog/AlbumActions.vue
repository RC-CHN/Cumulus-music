<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, shallowRef } from 'vue';
import { failure, type AccountView, type TrackView } from '../../services/contracts';
import type { CatalogMembershipListener } from '../../services/catalog';
import type { AlbumCardRequest } from '../../services/catalogCards';
import type { PlayerModel } from '../../composables/usePlayer';
import { useCore } from '../../services/core';
import MenuTransition from '../MenuTransition.vue';
import ModalTransition from '../ModalTransition.vue';
import TrackShareDialog from '../TrackShareDialog.vue';
import CatalogMembershipDialog from './CatalogMembershipDialog.vue';
const props = defineProps<{
  account: AccountView;
  player: PlayerModel;
  changed: CatalogMembershipListener;
}>();
const emit = defineEmits<{
  download: [tracks: TrackView[]];
  expired: [];
}>();
const core = useCore(),
  request = shallowRef<AlbumCardRequest>();
const shared = ref(false),
  membership = ref(false),
  message = ref(''),
  failed = ref(false),
  busy = ref(false);
const retrying = ref(false),
  feedbackRoot = ref<HTMLElement>();
let sequence = 0,
  timer: ReturnType<typeof setTimeout> | undefined;
function close() {
  sequence++;
  clearTimeout(timer);
  shared.value = false;
  membership.value = false;
  message.value = '';
  failed.value = false;
  busy.value = false;
  retrying.value = false;
  request.value = undefined;
}
onBeforeUnmount(close);
function feedback(value: string, error = false, hide = true) {
  clearTimeout(timer);
  message.value = value;
  failed.value = error;
  if (!error && hide)
    timer = setTimeout(() => {
      if (!feedbackRoot.value?.contains(document.activeElement)) message.value = '';
    }, 3500);
}
function dismiss() {
  const anchor = request.value?.anchor;
  close();
  if (anchor?.isConnected) anchor.focus({ preventScroll: true });
}
async function show(value: AlbumCardRequest) {
  close();
  request.value = value;
  if (value.action === 'share') shared.value = true;
  else if (value.action === 'collect' || value.action === 'remove') membership.value = true;
  else await execute();
}
async function execute() {
  const target = request.value;
  if (!target || busy.value || props.account.status !== 'authenticated') return;
  const current = sequence,
    epoch = props.account.epoch;
  const fromFeedback = !!feedbackRoot.value?.contains(document.activeElement);
  retrying.value = failed.value;
  busy.value = true;
  failed.value = false;
  message.value = fromFeedback ? '正在处理…' : '';
  clearTimeout(timer);
  try {
    if (target.action === 'next' || target.action === 'queue') {
      const starts = !props.player.view.value?.queueLen && !props.player.view.value?.track;
      const result = await props.player.queueAlbum(target.album.id, target.selection);
      if (!result) throw { code: 'playback_changed', message: '播放状态已变化，请重试' };
      if (current === sequence)
        feedback(
          starts
            ? '已开始播放'
            : target.action === 'queue'
              ? '已添加到播放列表'
              : '已设为下一首播放',
        );
    } else if (target.action === 'copy') {
      if (!/^\d+$/.test(target.album.id)) throw { code: 'invalid', message: '专辑链接暂不可用' };
      await core.copyText(`https://music.163.com/album?id=${target.album.id}`);
      if (current === sequence) feedback('链接已复制');
    } else if (target.action === 'download') {
      feedback('正在读取专辑歌曲…', false, false);
      const result = await core.catalogPage(epoch, { kind: 'album', id: target.album.id }, true);
      if (current !== sequence) return;
      if (
        result.epoch !== epoch ||
        result.value?.page.kind !== 'album' ||
        result.value.page.album.id !== target.album.id
      )
        throw { code: 'stale', message: '专辑已变化，请重新打开' };
      const tracks = result.value.page.tracks.filter((track) => !track.missing && !track.local);
      if (!tracks.length) throw { code: 'unavailable', message: '没有可下载的歌曲' };
      message.value = '';
      clearTimeout(timer);
      emit('download', tracks);
    }
  } catch (issue) {
    if (current !== sequence) return;
    const value = failure(issue);
    feedback(value.message, true);
    if (['session_expired', 'profile_changed'].includes(value.code)) emit('expired');
  } finally {
    if (current === sequence) {
      busy.value = false;
      retrying.value = false;
      await nextTick();
      if (fromFeedback && current === sequence && document.activeElement === document.body) {
        const anchor = target.anchor;
        (anchor?.isConnected
          ? anchor
          : document.querySelector<HTMLElement>('.collections-page h1,.catalog-heading h1')
        )?.focus({ preventScroll: true });
      }
    }
  }
}
defineExpose({ show, close });
const fallbackFocus = () =>
  document.querySelector<HTMLElement>('.collections-page h1,.catalog-heading h1');
</script>
<template>
  <ModalTransition :return-focus="() => request?.anchor ?? null" :fallback-focus="fallbackFocus"
    ><TrackShareDialog v-if="shared && request" :album="request.album" @close="shared = false"
  /></ModalTransition>
  <ModalTransition :return-focus="() => request?.anchor ?? null" :fallback-focus="fallbackFocus"
    ><CatalogMembershipDialog
      v-if="membership && request"
      :account="account"
      :resource="{ kind: 'album', id: request.album.id }"
      :name="request.album.name"
      :desired="request.action === 'collect'"
      :changed="changed"
      @close="membership = false"
      @expired="emit('expired')"
  /></ModalTransition>
  <MenuTransition
    ><div
      v-if="message"
      ref="feedbackRoot"
      class="track-action-feedback album-action-feedback"
      :class="{ failed }"
      role="status"
    >
      {{ message }}
      <button v-if="failed || retrying" class="text-button" :aria-disabled="busy" @click="execute">
        {{ busy ? '正在处理…' : '重试' }}
      </button>
      <button class="text-button" @click="dismiss">关闭</button>
    </div></MenuTransition
  >
</template>
