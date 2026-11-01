<script setup lang="ts">
import { computed, ref, watch, nextTick } from 'vue';
import type { CloudRemoval } from '../../services/cloud';
const props = defineProps<{
  view?: CloudRemoval;
  total: number;
  disabled: boolean;
  submitted: boolean;
}>();
const selected = defineModel<boolean>({ required: true });
const page = ref(0);
const root = ref<HTMLElement>(),
  list = ref<HTMLElement>();
const records = computed(() => props.view?.files.filter((f) => f.state !== 'skipped') ?? []);
const pages = computed(() => Math.max(1, Math.ceil(records.value.length / 20)));
const visible = computed(() => records.value.slice(page.value * 20, (page.value + 1) * 20));
watch(pages, (count) => {
  page.value = Math.min(page.value, count - 1);
});
const summary = computed(() => {
  const rows = records.value;
  const done = rows.filter((f) => ['removed', 'missing'].includes(f.state)).length;
  const kept = rows.filter((f) => f.state === 'retained').length;
  const failed = rows.filter((f) => f.state === 'failed').length;
  const pending = rows.filter((f) => ['pending', 'removing'].includes(f.state)).length;
  return `本地文件：已清理 ${done} 项${kept ? `，保留 ${kept} 项` : ''}${failed ? `，未完成 ${failed} 项` : ''}${pending ? `，${props.view?.phase === 'result' ? '未执行' : '待处理'} ${pending} 项` : ''}`;
});
function label(file: CloudRemoval['files'][number]) {
  if (file.state === 'pending' && props.view?.phase === 'result') return '未执行，文件已保留';
  return (
    file.issue ??
    {
      pending: '等待处理',
      removing: '正在处理',
      removed: '已删除',
      missing: '文件已不存在',
      retained: '已保留',
      failed: '未完成',
      skipped: '已保留',
    }[file.state]
  );
}
async function move(direction: number) {
  if (props.disabled) return;
  const origin = document.activeElement;
  page.value = Math.max(0, Math.min(pages.value - 1, page.value + direction));
  await nextTick();
  list.value?.scrollTo({ top: 0 });
  if (origin instanceof HTMLElement && origin.matches(':disabled'))
    root.value?.querySelector<HTMLElement>('button:not(:disabled)')?.focus();
}
</script>
<template>
  <div ref="root" class="cloud-delete-local">
    <label v-if="view && view.localCount > 0" class="cloud-delete-local-option">
      <input v-model="selected" type="checkbox" :disabled="disabled" data-cloud-delete-local />
      <span>同时删除本地音乐文件</span>
    </label>
    <p v-if="!submitted && view?.localCount" class="cloud-delete-local-note">
      包含本机上传源文件或已完成的云盘下载，删除后无法撤销。
    </p>
    <p v-if="view?.cueCount" class="cloud-delete-local-note" data-cloud-delete-cue>
      {{
        view.cueCount === total
          ? '所选歌曲均来自 CUE 分轨，本地音源将保留。'
          : `其中 ${view.cueCount} 首关联 CUE 分轨，本地音源将保留。`
      }}
    </p>
    <p v-if="!selected" class="cloud-delete-local-note">
      操作范围为当前账号的云盘资源，本地文件保留。
    </p>
    <p v-if="view?.localIssue" class="cloud-delete-local-note" role="status">
      {{ view.localIssue }}
    </p>
    <details
      v-if="submitted && view?.deleteLocal && records.length"
      class="cloud-delete-local-result"
    >
      <summary>{{ summary }}</summary>
      <ul ref="list" aria-label="本地文件处理结果" tabindex="0">
        <li v-for="file in visible" :key="file.cloudId">
          <span>{{ file.title }}</span
          ><span>{{ label(file) }}</span>
        </li>
      </ul>
      <nav v-if="pages > 1" class="cloud-delete-pagination" aria-label="本地结果分页">
        <button class="secondary" :disabled="page === 0 || disabled" @click="move(-1)">
          上一页
        </button>
        <span>{{ page + 1 }} / {{ pages }}</span>
        <button class="secondary" :disabled="page + 1 === pages || disabled" @click="move(1)">
          下一页
        </button>
      </nav>
    </details>
  </div>
</template>
