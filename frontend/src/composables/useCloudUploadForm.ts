import { computed, onScopeDispose, ref, watch } from 'vue';
import type { CloudModel } from './useCloud.ts';
import { failure } from '../services/contracts.ts';
import {
  uploadRequestId,
  validUploadReceipt,
  type CloudUploadRow,
} from '../services/cloudUploadForm.ts';
export function useCloudUploadForm(
  model: Pick<CloudModel, 'accountEpoch' | 'busy' | 'error' | 'pick' | 'command'>,
  callbacks: { started: () => void; close: () => void },
  identifier: () => string = uploadRequestId,
) {
  const rows = ref<CloudUploadRow[]>([]),
    choosing = ref(false),
    submitting = ref(false),
    uncertain = ref(false),
    error = ref(''),
    message = ref(''),
    page = ref(0);
  const owner = model.accountEpoch.value;
  let alive = true,
    serial = 0;
  const active = () => alive && owner === model.accountEpoch.value && !!owner;
  const blocked = computed(
    () => !active() || model.busy.value || choosing.value || submitting.value,
  );
  const editable = computed(() => !blocked.value && !uncertain.value);
  const pageSize = 20,
    visible = computed(() => rows.value.slice(page.value * pageSize, (page.value + 1) * pageSize));
  watch(
    () => model.accountEpoch.value,
    () => {
      if (!active()) {
        serial++;
        rows.value = [];
        error.value = '';
        message.value = '';
        uncertain.value = false;
        choosing.value = false;
        submitting.value = false;
      }
    },
    { flush: 'sync' },
  );
  watch(
    () => rows.value.length,
    () => {
      page.value = Math.min(page.value, Math.max(0, Math.ceil(rows.value.length / pageSize) - 1));
    },
  );
  onScopeDispose(() => {
    alive = false;
    serial++;
    rows.value = [];
  });
  async function pick(kind: 'audio' | 'folder') {
    if (!editable.value) return;
    const attempt = ++serial;
    choosing.value = true;
    error.value = '';
    model.error.value = '';
    message.value = '';
    try {
      const paths = await model.pick(kind);
      if (!active() || attempt !== serial) return;
      const added = [...new Set(paths)].filter(
        (path) => !rows.value.some((r) => r.input.path === path),
      );
      if (rows.value.length + added.length > 500) {
        error.value = '单次最多添加 500 个文件，请减少选择。';
        return;
      }
      rows.value.push(
        ...added.map((path) => ({
          requestId: identifier(),
          input: { path, cover: null, lyric: null, cue: null },
          error: '',
        })),
      );
      if (paths.length && !added.length) message.value = '所选文件已在列表中。';
    } catch (e) {
      if (active() && attempt === serial) error.value = failure(e).message;
    } finally {
      if (active() && attempt === serial) choosing.value = false;
    }
  }
  async function attachment(row: CloudUploadRow, kind: 'cover' | 'lyric' | 'cue') {
    if (!editable.value || !rows.value.includes(row)) return;
    const attempt = ++serial;
    choosing.value = true;
    error.value = '';
    model.error.value = '';
    message.value = '';
    try {
      const [path] = await model.pick(kind);
      if (
        active() &&
        attempt === serial &&
        path &&
        rows.value.includes(row) &&
        row.input[kind] !== path
      ) {
        row.input[kind] = path;
        row.requestId = identifier();
        row.error = '';
      }
    } catch (e) {
      if (active() && attempt === serial) error.value = failure(e).message;
    } finally {
      if (active() && attempt === serial) choosing.value = false;
    }
  }
  function clearAttachment(row: CloudUploadRow, kind: 'cover' | 'lyric' | 'cue') {
    if (!editable.value || !rows.value.includes(row)) return;
    row.input[kind] = null;
    row.requestId = identifier();
    row.error = '';
    error.value = '';
    message.value = '';
  }
  function remove(row: CloudUploadRow) {
    if (editable.value) {
      rows.value = rows.value.filter((r) => r !== row);
      error.value = '';
      message.value = '';
    }
  }
  async function submit() {
    if (blocked.value || !rows.value.length) return;
    const attempt = ++serial,
      submitted = rows.value.map((row) => ({ ...row, input: { ...row.input } }));
    submitting.value = true;
    error.value = '';
    model.error.value = '';
    message.value = '正在登记上传任务…';
    try {
      const result = await model.command({
        kind: 'upload_planned',
        files: submitted.map((row) => ({ request_id: row.requestId, input: row.input })),
      });
      if (!active() || attempt !== serial) return;
      if (!result || !validUploadReceipt(result, submitted, owner)) {
        uncertain.value = true;
        message.value = '';
        error.value = '添加结果尚未确认，请核对并补全任务。已登记的文件不会重复添加。';
        return;
      }
      uncertain.value = false;
      const rejected = new Map(result.failed.map((f) => [f.id, f.message]));
      rows.value = rows.value
        .filter((row) => rejected.has(row.input.path))
        .map((row) => ({ ...row, error: rejected.get(row.input.path)! }));
      page.value = 0;
      const registration = `已登记 ${result.accepted.length} 个任务${result.failed.length ? `，${result.failed.length} 个文件未完成，请检查下方原因。` : '。'}${result.notStarted?.length ? `其中 ${result.notStarted.length} 个尚未开始，请到正在上传中处理。` : ''}`;
      if (result.uploadLimit === 'no_new_tasks') {
        error.value = '文件超过100M，无法上传';
        message.value = result.accepted.length ? registration : '';
      } else {
        message.value =
          result.uploadLimit === 'partial'
            ? `部分文件超过100M，已经过滤。${registration}`
            : registration;
      }
      if (result.accepted.length) callbacks.started();
      if (!result.failed.length) callbacks.close();
    } catch (e) {
      if (active() && attempt === serial) {
        uncertain.value = true;
        error.value = failure(e).message;
        message.value = '添加结果尚未确认；核对会复用本次任务，不重复添加已登记的文件。';
      }
    } finally {
      if (active() && attempt === serial) submitting.value = false;
    }
  }
  return {
    owner,
    rows,
    visible,
    page,
    pageSize,
    choosing,
    submitting,
    uncertain,
    error,
    message,
    blocked,
    editable,
    active,
    pick,
    attachment,
    clearAttachment,
    remove,
    submit,
  };
}
