<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import ActionMenu from '../components/ActionMenu.vue';
import type { MiniView } from '../services/mini';
import type { MenuItem, MenuLeaf } from '../services/menu';
import type { useMiniModes } from '../composables/useMiniModes';
import type { useMiniWindowMenu } from '../composables/useMiniWindowMenu';
import { modeIcon, modeLabel } from '../services/playbackMode';
const props = defineProps<{
  view: MiniView | null;
  modes: ReturnType<typeof useMiniModes>;
  lyrics: ReturnType<typeof useMiniWindowMenu>;
  pinLabel: string;
  pinSelected: boolean | undefined;
  disabled: boolean;
}>();
const emit = defineEmits<{ select: [key: string] }>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const opened = ref(false);
watch(
  [
    () => props.view?.media.epoch,
    () => props.view?.media.queueRevision,
    () => props.view?.media.modeFamily,
    () => props.view?.media.selection.audioEntryId,
    () => props.view?.media.selection.audioPlayId,
  ],
  (next, previous) => {
    if (opened.value) menu.value?.close(next[0] === previous[0] && menu.value.containsFocus());
  },
);
function open() {
  opened.value = true;
  void props.lyrics.read();
}
function close() {
  opened.value = false;
  props.lyrics.closed();
}
const items = computed<MenuItem[]>(() => {
  const media = props.view?.media,
    lyric = props.lyrics.lyrics.value;
  const result: MenuItem[] = [
    {
      key: 'pin',
      label: props.pinLabel,
      checked: props.pinSelected,
      checkKind: 'checkbox',
      icon: 'pin',
    },
    { key: 'return', label: '完整模式', icon: 'monitor' },
  ];
  if (media?.mode && media.modeFamily) {
    const family = props.modes.mode.family.value;
    const choices: MenuLeaf[] = [];
    if (family === 'intelligence' || (family === 'ordinary' && media.canIntelligence))
      choices.push({
        key: 'mode:intelligence',
        label: '心动模式',
        icon: 'heartbeat',
        checked: family === 'intelligence',
      });
    for (const mode of family === 'fm'
      ? (['sequential', 'single_loop'] as const)
      : family === 'together'
        ? (['order_loop', 'single_loop', 'random'] as const)
        : (['order_loop', 'single_loop', 'random', 'sequential'] as const))
      choices.push({
        key: `mode:${mode}`,
        label: modeLabel(mode, family === 'intelligence' ? 'ordinary' : family),
        icon: modeIcon(mode, family === 'intelligence' ? 'ordinary' : family),
        checked: family !== 'intelligence' && media.mode === mode,
      });
    result.push({
      key: 'mode',
      label: modeLabel(media.mode, family),
      icon: modeIcon(media.mode, family),
      children: choices,
    });
  }
  result.push({ key: 'lyrics-separator', label: '', separator: true });
  if (lyric) {
    if (lyric.damaged)
      result.push({ key: 'lyric:repair', label: '修复桌面歌词设置', icon: 'refresh' });
    else {
      if (lyric.enabled && !lyric.visible && !lyric.opening)
        result.push({ key: 'lyric:retry', label: '重试打开桌面歌词', icon: 'refresh' });
      result.push({
        key: 'lyric:toggle',
        label: lyric.enabled ? '关闭桌面歌词' : '打开桌面歌词',
        icon: 'music',
        disabled: lyric.opening,
      });
      if (lyric.visible && !lyric.locked)
        result.push({
          key: lyric.locked ? 'lyric:unlock' : 'lyric:lock',
          label: lyric.locked ? '解锁桌面歌词' : '锁定桌面歌词',
          icon: lyric.locked ? 'unlock' : 'lock',
        });
    }
    if (lyric.visible && lyric.locked)
      result.push({ key: 'lyric:unlock', label: '解锁桌面歌词', icon: 'unlock' });
  } else if (props.lyrics.readIssue.value)
    result.push({ key: 'lyric:read', label: '重新读取歌词状态', icon: 'refresh' });
  result.push(
    { key: 'exit-separator', label: '', separator: true },
    { key: 'exit', label: '退出 Cumulus', icon: 'close' },
  );
  return result;
});
async function select(key: string) {
  if (key === 'lyric:read') {
    await menu.value?.show();
    return;
  }
  emit('select', key);
}
defineExpose({
  showAt: (x: number, y: number, target: HTMLElement) => menu.value?.showAt(x, y, target),
  close: () => menu.value?.close(),
});
</script>
<template>
  <ActionMenu
    ref="menu"
    label="小窗菜单"
    icon="more"
    variant="mini"
    :items="items"
    :disabled="disabled"
    :data-lyric-context="lyrics.lyrics.value?.ticket"
    @opened="open"
    @closed="close"
    @select="select"
  >
    <template #header>
      <p v-if="lyrics.loading.value" class="mini-menu-status" role="status">正在读取歌词状态…</p>
      <p v-else-if="lyrics.readIssue.value" class="mini-menu-status failed" role="alert">
        {{ lyrics.readIssue.value }}
      </p>
      <p v-else-if="lyrics.lyrics.value?.error" class="mini-menu-status failed" role="alert">
        {{ lyrics.lyrics.value.error }}
      </p>
    </template>
  </ActionMenu>
</template>
<style scoped>
.mini-menu-status {
  font-size: 11px;
  line-height: 16px;
  margin: 4px 8px;
  color: var(--muted);
  overflow-wrap: anywhere;
}
.mini-menu-status.failed {
  color: var(--accent);
}
</style>
