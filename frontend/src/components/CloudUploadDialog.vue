<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import type { CloudModel } from '../composables/useCloud';
import type { CloudUploadInput } from '../services/cloud';
import Icon from './Icon.vue';
const props = defineProps<{ model: CloudModel }>();
const emit = defineEmits<{ close: []; started: [] }>();
const { busy, error } = props.model;
const files = ref<CloudUploadInput[]>([]),
  choosing = ref(false),
  dialog = ref<HTMLDialogElement>();
const blocked = computed(() => busy.value || choosing.value);
let focus: HTMLElement | null = null;
onMounted(() => {
  focus = document.activeElement as HTMLElement;
  dialog.value?.showModal();
  error.value = '';
});
onUnmounted(() => focus?.focus());
function basename(path: string | null) {
  return path?.split(/[\\/]/).pop() ?? '未选择';
}
async function pick(kind: 'audio' | 'folder' = 'audio') {
  choosing.value = true;
  try {
    const paths = await props.model.pick(kind);
    for (const path of paths)
      if (!files.value.some((f) => f.path === path))
        files.value.push({ path, cover: null, lyric: null, cue: null });
  } finally {
    choosing.value = false;
  }
}
async function attachment(file: CloudUploadInput, kind: 'cover' | 'lyric' | 'cue') {
  choosing.value = true;
  try {
    const [path] = await props.model.pick(kind);
    if (path) file[kind] = path;
  } finally {
    choosing.value = false;
  }
}
async function upload() {
  const result = await props.model.command({
    kind: 'upload',
    files: files.value.map((f) => ({ ...f })),
  });
  if (result?.accepted.length) emit('started');
  if (result && !result.failed.length) emit('close');
  else if (result?.accepted.length) {
    const failed = new Set(result.failed.map((f) => f.id));
    files.value = files.value.filter((f) => failed.has(f.path));
  }
}
function close() {
  if (!blocked.value) emit('close');
}
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog cloud-upload-dialog"
    aria-labelledby="cloud-upload-title"
    @cancel.prevent="close"
  >
    <header>
      <h2 id="cloud-upload-title">上传音乐</h2>
      <button class="icon-button" aria-label="关闭上传窗口" :disabled="blocked" @click="close">
        <Icon name="close" />
      </button>
    </header>
    <p class="muted">选择本地音频，上传后可在登录此账号的设备上收听。每个文件最大 512 MB。</p>
    <div class="cloud-source-actions">
      <button class="secondary" :disabled="blocked" @click="pick('audio')">
        <Icon name="up" />选择音频文件
      </button>
      <button class="secondary" :disabled="blocked" @click="pick('folder')">选择文件夹</button>
    </div>
    <div class="cloud-file-list">
      <article v-for="(file, index) in files" :key="file.path">
        <div class="cloud-file-title">
          <strong :title="file.path">{{ basename(file.path) }}</strong
          ><button class="text-button" :disabled="blocked" @click="files.splice(index, 1)">
            移除
          </button>
        </div>
        <details>
          <summary>附加封面、歌词或 CUE</summary>
          <div
            v-for="entry in [
              { kind: 'cover', name: '封面' },
              { kind: 'lyric', name: '歌词' },
              { kind: 'cue', name: 'CUE' },
            ] as const"
            :key="entry.kind"
            class="cloud-attachment"
          >
            <span>{{ entry.name }}</span
            ><span class="ellipsis" :title="file[entry.kind] ?? ''">{{
              basename(file[entry.kind])
            }}</span
            ><button class="text-button" :disabled="blocked" @click="attachment(file, entry.kind)">
              选择</button
            ><button
              v-if="file[entry.kind]"
              class="text-button"
              :disabled="blocked"
              @click="file[entry.kind] = null"
            >
              清除
            </button>
          </div>
        </details>
      </article>
    </div>
    <p v-if="error" role="alert" class="cloud-error">{{ error }}</p>
    <footer>
      <button class="secondary" :disabled="blocked" @click="close">取消</button
      ><button class="primary" :disabled="blocked || !files.length" @click="upload">
        {{ busy ? '正在添加…' : `开始上传${files.length ? ` (${files.length})` : ''}` }}
      </button>
    </footer>
  </dialog>
</template>
