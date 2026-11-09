<script setup lang="ts">
import { computed, onMounted, onScopeDispose, ref } from 'vue';
import { useLocalDirectories } from '../composables/useLocalDirectories';
import { useCore } from '../services/core';
import Icon from './Icon.vue';

const props = defineProps<{ current: () => boolean }>();
const emit = defineEmits<{ close: []; applied: [] }>();
const dialog = ref<HTMLDialogElement>();
const model = useLocalDirectories(useCore(), props.current);
const { snapshot, draft, busy, error, stale, usable } = model;
const blockingClose = computed(() => busy.value === 'pick' || busy.value === 'apply');
onMounted(() => {
  dialog.value?.showModal();
  void model.read();
});
onScopeDispose(model.dispose);
function close() {
  if (!blockingClose.value) emit('close');
}
async function confirm() {
  const result = await model.confirm();
  if (!result) return;
  if (result.changed) emit('applied');
  emit('close');
}
</script>

<template>
  <dialog
    ref="dialog"
    class="playlist-dialog local-directories-dialog"
    data-local-directories
    aria-labelledby="local-directories-heading"
    :aria-busy="!!busy"
    @cancel.prevent="close"
  >
    <header>
      <h2 id="local-directories-heading">选择本地文件夹</h2>
      <button
        class="icon-button"
        aria-label="关闭目录选择"
        :disabled="blockingClose"
        autofocus
        @click="close"
      >
        <Icon name="close" />
      </button>
    </header>
    <div class="local-directory-content">
      <p>将自动扫描勾选的目录。取消勾选后，确认将从列表中移除该目录的音乐，源文件保留。</p>
      <p v-if="busy === 'read'" role="status">正在读取本地文件夹…</p>
      <p v-else-if="snapshot && !snapshot.entries.length">尚未添加本地文件夹。</p>
      <div v-for="entry in snapshot?.entries" :key="entry.id" class="local-directory-option">
        <label :title="entry.path">
          <input
            type="checkbox"
            :checked="draft.get(entry.id)"
            :disabled="!usable"
            @change="model.change(entry.id, ($event.target as HTMLInputElement).checked)"
          />
          <span>{{ entry.path }}<small v-if="entry.paused">（已暂停自动扫描）</small></span>
        </label>
      </div>
      <p v-if="error" class="error-banner" role="alert">
        {{ error }}
        <button
          v-if="stale || !snapshot"
          class="text-button"
          :disabled="!!busy"
          @click="model.read"
        >
          重新读取
        </button>
      </p>
    </div>
    <footer>
      <button class="secondary" :disabled="!usable" @click="model.add">添加文件夹</button>
      <button class="primary" :disabled="!usable" @click="confirm">确认</button>
    </footer>
  </dialog>
</template>

<style scoped>
.local-directories-dialog[open] {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.local-directories-dialog header,
.local-directories-dialog footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}
.local-directories-dialog h2 {
  margin: 0;
}
.local-directories-dialog footer {
  justify-content: flex-end;
}
.local-directory-content {
  min-height: 0;
  overflow: auto;
  overflow-wrap: anywhere;
}
.local-directory-content p {
  margin: 0 0 16px;
  color: var(--muted);
}
.local-directory-option {
  padding-block: 8px;
}
.local-directory-option label {
  display: flex;
  align-items: flex-start;
  gap: 10px;
}
.local-directory-option label small {
  color: var(--muted);
}
</style>
