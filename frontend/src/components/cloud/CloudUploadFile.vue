<script setup lang="ts">
import { useId } from 'vue';
import { uploadBasename, type CloudUploadRow } from '../../services/cloudUploadForm';
defineProps<{ row: CloudUploadRow; disabled: boolean }>();
const emit = defineEmits<{
  remove: [event: MouseEvent];
  choose: [kind: 'cover' | 'lyric' | 'cue', event: MouseEvent];
  clear: [kind: 'cover' | 'lyric' | 'cue', event: MouseEvent];
}>();
const issueId = useId();
</script>
<template>
  <article
    data-upload-file
    :data-upload-path="row.input.path"
    :aria-describedby="row.error ? issueId : undefined"
  >
    <div class="cloud-file-title">
      <strong :title="row.input.path">{{ uploadBasename(row.input.path) }}</strong>
      <button
        class="text-button cloud-file-remove"
        :aria-label="`移除 ${uploadBasename(row.input.path)}`"
        :disabled="disabled"
        @click="emit('remove', $event)"
      >
        移除
      </button>
    </div>
    <p class="cloud-source-path" :title="row.input.path">{{ row.input.path }}</p>
    <p
      v-if="row.error"
      :id="issueId"
      class="cloud-error cloud-file-error"
      role="alert"
      tabindex="-1"
    >
      {{ row.error }}
    </p>
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
        :data-attachment-kind="entry.kind"
      >
        <span>{{ entry.name }}</span
        ><span class="ellipsis" :title="row.input[entry.kind] ?? ''">{{
          uploadBasename(row.input[entry.kind])
        }}</span>
        <button
          class="text-button"
          :aria-label="`选择${entry.name}：${uploadBasename(row.input.path)}`"
          :disabled="disabled"
          @click="emit('choose', entry.kind, $event)"
        >
          选择
        </button>
        <button
          v-if="row.input[entry.kind]"
          class="text-button"
          :aria-label="`清除${entry.name}：${uploadBasename(row.input.path)}`"
          :disabled="disabled"
          @click="emit('clear', entry.kind, $event)"
        >
          清除
        </button>
      </div>
    </details>
  </article>
</template>
