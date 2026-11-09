<script setup lang="ts">
import { computed, nextTick, onMounted, onScopeDispose, ref, shallowRef, watch } from 'vue';
import { useCore } from '../services/core';
import { failure } from '../services/contracts';
import type { MenuItem } from '../services/menu';
import {
  podcastProgramPublicLink,
  type EpisodeView,
  type PodcastPage,
  type PodcastProgramTarget,
} from '../services/podcast';
import ActionMenu from './ActionMenu.vue';
import ModalTransition from './ModalTransition.vue';
import TrackShareDialog from './TrackShareDialog.vue';

const props = defineProps<{
  page?: PodcastPage;
  playable: boolean;
  queueable: boolean;
  queue: (epoch: string, program: PodcastProgramTarget) => Promise<boolean>;
}>();
const emit = defineEmits<{
  play: [id: string];
  comments: [id: string, title: string];
  download: [voices: EpisodeView[]];
}>();
const client = useCore();
const menu = ref<InstanceType<typeof ActionMenu>>();
const opened = ref(false);
const count = ref<number>();
const completed = ref<boolean>();
const feedback = ref('');
let timer: ReturnType<typeof setTimeout> | undefined;
type Selection = {
  page: PodcastPage;
  voice: EpisodeView;
  target: PodcastProgramTarget;
  anchor: HTMLButtonElement;
};
const selection = shallowRef<Selection>();
const shared = shallowRef<Selection>();
const shareAnchor = shallowRef<HTMLButtonElement>();
let preparing: Selection | undefined;
function current(value: Selection) {
  return props.page === value.page && value.anchor.isConnected && !value.anchor.closest('[inert]');
}
const active = computed(() =>
  selection.value && current(selection.value) ? selection.value : undefined,
);
const items = computed<MenuItem[]>(() => {
  const voice = active.value?.voice;
  const playable = !!voice && !voice.unavailable;
  const publicLink = voice && podcastProgramPublicLink(voice.id);
  return [
    { key: 'play', label: '播放', icon: 'play', disabled: !props.playable || !playable },
    { key: 'next', label: '下一首播放', icon: 'next', disabled: !props.queueable || !playable },
    {
      key: 'comments',
      label: count.value ? `查看评论(${count.value})` : '查看评论',
      icon: 'comment',
    },
    { key: 'sep', label: '', separator: true },
    ...(completed.value === true
      ? []
      : [
          {
            key: 'download',
            label: '下载',
            icon: 'down',
            disabled: !playable || completed.value === undefined,
          },
        ]),
    { key: 'share', label: '分享...', icon: 'share', disabled: !publicLink },
    { key: 'copy', label: '复制链接', icon: 'copy', disabled: !publicLink },
    ...(completed.value === true
      ? [{ key: 'folder', label: '打开文件所在目录', icon: 'folder' }]
      : []),
  ];
});
function clearFeedback() {
  clearTimeout(timer);
  feedback.value = '';
}
function abandonPreparation() {
  if (preparing && selection.value === preparing) selection.value = undefined;
  preparing = undefined;
}
function abandonForFocus(event: FocusEvent) {
  if (preparing && (!(event.target instanceof Node) || !preparing.anchor.contains(event.target)))
    abandonPreparation();
}
watch(
  () => props.page,
  () => {
    abandonPreparation();
    selection.value = undefined;
    shared.value = undefined;
    shareAnchor.value = undefined;
    menu.value?.close();
    clearFeedback();
  },
  { flush: 'sync' },
);
onMounted(() => {
  document.addEventListener('pointerdown', abandonPreparation, true);
  document.addEventListener('keydown', abandonPreparation, true);
  document.addEventListener('focusin', abandonForFocus, true);
  window.addEventListener('scroll', abandonPreparation, true);
  window.addEventListener('resize', abandonPreparation);
  window.addEventListener('blur', abandonPreparation);
});
onScopeDispose(() => {
  abandonPreparation();
  selection.value = undefined;
  clearTimeout(timer);
  document.removeEventListener('pointerdown', abandonPreparation, true);
  document.removeEventListener('keydown', abandonPreparation, true);
  document.removeEventListener('focusin', abandonForFocus, true);
  window.removeEventListener('scroll', abandonPreparation, true);
  window.removeEventListener('resize', abandonPreparation);
  window.removeEventListener('blur', abandonPreparation);
});

async function open(
  voice: EpisodeView,
  anchor: HTMLButtonElement,
  point?: { x: number; y: number },
) {
  const page = props.page;
  if (
    !page?.epoch ||
    page.scope.kind !== 'radio' ||
    !page.episodes.includes(voice) ||
    !anchor.isConnected ||
    anchor.closest('[inert]')
  )
    return;
  if (!point && opened.value && selection.value?.anchor === anchor) {
    menu.value?.close(true);
    return;
  }
  abandonPreparation();
  menu.value?.close();
  anchor.focus({ preventScroll: true });
  const value: Selection = {
    page,
    voice,
    anchor,
    target: {
      radioId: page.scope.id,
      ascending: page.scope.ascending,
      revision: page.revision,
      voiceId: voice.id,
    },
  };
  clearFeedback();
  count.value = undefined;
  completed.value = undefined;
  selection.value = value;
  preparing = value;
  const live = () => preparing === value && selection.value === value && current(value);
  try {
    // The official menu prepares the optional count before consulting download
    // state and presenting the actions. Native returns null on a network miss,
    // but rejects a retired account or radio revision.
    const result = await client.podcastProgramCommentCount(page.epoch, value.target);
    if (!live() || result.epoch !== page.epoch) return;
    const status = await client.voiceDownloadState(page.epoch, voice.id).catch(() => undefined);
    if (!live()) return;
    count.value = result.value ?? undefined;
    completed.value = status === undefined ? undefined : status === 'complete';
    await nextTick();
    if (!live()) return;
    preparing = undefined;
    if (point) await menu.value?.showAt(point.x, point.y, anchor);
    else await menu.value?.showBelow(anchor);
  } catch {
    // A rejected scope must not materialize a menu for the old row.
  } finally {
    if (preparing === value) abandonPreparation();
  }
}
async function action(key: string) {
  const value = active.value;
  if (!value) return;
  const { page, voice, anchor, target } = value;
  if (key === 'play' && props.playable && !voice.unavailable) emit('play', voice.id);
  if (key === 'comments') emit('comments', voice.id, voice.name);
  if (key === 'download' && completed.value === false && !voice.unavailable)
    emit('download', [voice]);
  if (key === 'share' && podcastProgramPublicLink(voice.id)) {
    shareAnchor.value = anchor;
    shared.value = value;
  }
  let message: string | undefined;
  try {
    if (key === 'next' && props.queueable && !voice.unavailable) {
      if (await props.queue(page.epoch, target)) message = '已添加至播放列表';
    }
    if (key === 'copy') {
      const link = podcastProgramPublicLink(voice.id);
      if (!link) return;
      await client.copyText(link);
      message = '链接已复制';
    }
    if (key === 'folder' && completed.value === true)
      await client.podcastOpenProgramDownloadDirectory(page.epoch, target);
  } catch (error) {
    message = failure(error).message;
  }
  if (message && current(value) && selection.value === value) {
    clearFeedback();
    feedback.value = message;
    timer = setTimeout(clearFeedback, 3500);
  }
}
defineExpose({ open });
</script>

<template>
  <ActionMenu
    ref="menu"
    context-only
    variant="song"
    :label="`单集菜单：${active?.voice.name ?? ''}`"
    :disabled="!active"
    :items="items"
    @opened="opened = true"
    @closed="opened = false"
    @select="action"
  />
  <p v-if="feedback" class="form-success" role="status" data-podcast-program-status>
    {{ feedback }}
  </p>
  <ModalTransition :return-focus="() => shareAnchor ?? null" :restore-focus="!!page">
    <TrackShareDialog v-if="shared" :program="shared.voice" @close="shared = undefined" />
  </ModalTransition>
</template>
