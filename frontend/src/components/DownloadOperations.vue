<script setup lang="ts">
import { computed } from 'vue';
import type { DownloadModel } from '../composables/useDownloads';
import { bytesLabel } from '../services/downloads';
const props = defineProps<{ model: DownloadModel }>();
const { view, busy } = props.model;
const running = (id: string) => view.value?.jobs.some((j) => j.id === id && j.running);
const error = (id: string) => view.value?.jobs.find((j) => j.id === id)?.error;
const migrationProgress = computed(
  () => view.value?.migration?.items.filter((i) => i.phase !== 'pending').length ?? 0,
);
</script>
<template>
  <div
    v-if="view?.migration || view?.operations.length || view?.jobs.some((j) => j.error)"
    class="download-operations"
  >
    <article v-if="view?.migration" class="download-operation">
      <strong>{{
        view.migration.phase === 'rolling_back'
          ? '正在撤销目录迁移'
          : view.migration.phase === 'quiescing'
            ? '等待下载任务暂停'
            : '下载目录迁移'
      }}</strong>
      <p class="ellipsis" :title="view.migration.target">
        {{ view.migration.source }} → {{ view.migration.target }}
      </p>
      <p>
        {{ migrationProgress }} / {{ view.migration.items.length }} 项 · 预计占用
        {{ bytesLabel(view.migration.required_bytes)
        }}<span v-if="view.migration.waiting.length">
          · 等待 {{ view.migration.waiting.length }} 项停止</span
        ><span v-if="view.migration.skipped.length">
          · {{ view.migration.skipped.length }} 项不在原目录，保留原位置</span
        >
      </p>
      <p v-if="error(view.migration.id) || view.migration.error" class="form-warning">
        {{ error(view.migration.id) || '迁移尚未完成，请检查文件冲突和剩余空间' }}
      </p>
      <button
        v-if="running(view.migration.id)"
        class="text-button"
        :disabled="busy"
        @click="model.command({ kind: 'stop_operation', operation_id: view.migration.id })"
      >
        暂停迁移
      </button>
      <button
        v-else
        class="text-button"
        :disabled="busy"
        @click="
          model.command({ kind: 'migration', operation_id: view.migration.id, cancel: false })
        "
      >
        继续迁移
      </button>
      <button
        class="text-button"
        :disabled="busy || view.migration.phase === 'rolling_back'"
        @click="model.command({ kind: 'migration', operation_id: view.migration.id, cancel: true })"
      >
        撤销迁移
      </button>
    </article>
    <template v-for="operation in view?.operations" :key="operation.id">
      <article
        v-if="!view?.migration && ['prepared', 'cleanup'].includes(operation.phase)"
        class="download-operation"
      >
        <strong
          >{{
            operation.action === 'move'
              ? '移动文件'
              : operation.action === 'delete'
                ? '删除文件'
                : '移出下载列表'
          }}
          · {{ operation.phase === 'cleanup' ? '整理原目录' : '准备文件' }}</strong
        >
        <p v-for="file in operation.files" :key="file.source" class="ellipsis" :title="file.source">
          {{ file.source }} · {{ bytesLabel(file.bytes_copied) }}
        </p>
        <p v-if="error(operation.id) || operation.error" class="form-warning">
          {{ error(operation.id) || '操作尚未完成，请检查文件冲突和剩余空间' }}
        </p>
        <button
          v-if="running(operation.id)"
          class="text-button"
          :disabled="busy"
          @click="model.command({ kind: 'stop_operation', operation_id: operation.id })"
        >
          暂停
        </button>
        <button
          v-else
          class="text-button"
          :disabled="busy"
          @click="
            model.command({ kind: 'file_operation', operation_id: operation.id, cancel: false })
          "
        >
          继续
        </button>
        <button
          v-if="operation.phase === 'prepared'"
          class="text-button"
          :disabled="busy"
          @click="
            model.command({ kind: 'file_operation', operation_id: operation.id, cancel: true })
          "
        >
          撤销
        </button>
      </article>
      <p
        v-if="operation.old_work_retained || operation.files.some((f) => f.replacement_retained)"
        class="form-warning"
      >
        目录中有被其他程序更改的文件，已保留，请在文件管理器中核对。
      </p>
    </template>
    <p
      v-for="job in view?.jobs.filter((j) => j.kind === 'repair' && j.error)"
      :key="job.id"
      class="form-warning"
    >
      {{ job.error }}
    </p>
  </div>
</template>
