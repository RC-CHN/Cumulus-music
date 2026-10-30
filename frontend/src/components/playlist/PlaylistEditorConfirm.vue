<script setup lang="ts">
import { onMounted, ref } from 'vue';
const props = defineProps<{
  kind: 'leave' | 'delete' | 'clear' | 'publish';
  name: string;
  count: number;
  pending: boolean;
  busy: boolean;
}>();
const emit = defineEmits<{ cancel: []; confirm: [] }>();
const dialog = ref<HTMLDialogElement>();
onMounted(() => dialog.value?.showModal());
const titles = { leave: '离开编辑？', delete: '删除歌单', clear: '清空歌曲', publish: '公开歌单' };
const actions = {
  leave: '放弃修改并离开',
  delete: '确认删除',
  clear: '确认清空',
  publish: '确认公开',
};
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog playlist-editor-confirm"
    aria-labelledby="playlist-editor-confirm-title"
    @cancel.prevent="!busy && emit('cancel')"
  >
    <header>
      <h2 id="playlist-editor-confirm-title">{{ titles[kind] }}</h2>
    </header>
    <p v-if="kind === 'leave'">
      {{
        pending
          ? '歌单操作结果还未确认。离开后可在歌单操作记录中继续核对。'
          : '名称、简介或标签有未保存的修改。离开后将丢弃这些文字修改，已保存的封面不受影响。'
      }}
    </p>
    <p v-else-if="kind === 'clear'">
      清空「{{ name }}」中的 {{ count }} 首歌曲？音乐文件和播放队列将保留。
    </p>
    <p v-else-if="kind === 'delete'">删除「{{ name }}」？该歌单将从你的账号中移除。</p>
    <p v-else>将「{{ name }}」设为公开歌单？其他用户将可以查看和收藏。</p>
    <footer>
      <button class="download-button" autofocus :disabled="busy" @click="emit('cancel')">
        {{ kind === 'leave' ? '继续编辑' : '返回编辑' }}
      </button>
      <button class="primary" :disabled="busy" @click="emit('confirm')">
        {{ busy ? '正在处理…' : pending && kind === 'leave' ? '离开并稍后核对' : actions[kind] }}
      </button>
    </footer>
  </dialog>
</template>
