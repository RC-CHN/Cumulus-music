<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from 'vue';
import { failure, type StyleCategory, type StyleTagOperation } from '../services/contracts';
import { useCore } from '../services/core';
import Icon from './Icon.vue';

const props = defineProps<{ epoch: string; applied?: StyleCategory; revision?: string | null }>();
const emit = defineEmits<{ refreshed: []; expired: [] }>();
const core = useCore();
const trigger = ref<HTMLButtonElement>();
const dialog = ref<HTMLDialogElement>();
const open = ref(false);
const loading = ref(false);
const error = ref('');
const categories = ref<StyleCategory[]>([]);
const selected = ref<string>();
const draft = ref<{ categoryId: string; tagId: string }>();
const operation = ref<StyleTagOperation>();
const attempted = ref<string>();
const readingOperations = ref(false);
const operationsKnown = ref(false);
const submitting = ref(false);
const feedback = ref('');
const unresolved = computed(
  () => operation.value?.phase === 'pending' || operation.value?.phase === 'uncertain',
);
const observedNames = computed(() => {
  const observed = operation.value?.observed;
  const group = categories.value.find((item) => item.id === observed?.categoryId);
  if (!group || !observed?.tagIds.length) return;
  const names = observed.tagIds.map((id) => group.tags.find((tag) => tag.id === id)?.name);
  if (names.every(Boolean)) return `${group.name} · ${names.join('、')}`;
});
function selectionLabel(selection?: { categoryId: string; tagId: string }) {
  const group = categories.value.find((item) => item.id === selection?.categoryId);
  const tag = group?.tags.find((item) => item.id === selection?.tagId);
  if (group && tag) return `${group.name} · ${tag.name}`;
}
const draftLabel = computed(() => selectionLabel(draft.value));
const unresolvedLabel = computed(() => selectionLabel(operation.value));
const canSave = computed(
  () =>
    !!props.revision &&
    !!props.applied &&
    !!draft.value &&
    !!draftLabel.value &&
    operationsKnown.value &&
    !readingOperations.value &&
    !submitting.value &&
    !attempted.value &&
    !unresolved.value &&
    !(
      props.applied.id === draft.value.categoryId &&
      props.applied.tags.length === 1 &&
      props.applied.tags[0]?.id === draft.value.tagId
    ),
);
const category = computed(() => categories.value.find((item) => item.id === selected.value));
const appliedIds = computed(() =>
  props.applied?.id === category.value?.id
    ? new Set(props.applied?.tags.map((tag) => tag.id))
    : new Set<string>(),
);
let request = 0;
let dialogRun = 0;

function report(issue: unknown) {
  const error = failure(issue);
  feedback.value = error.message;
  if (error.code === 'session_expired' || error.code === 'profile_changed') emit('expired');
}
function publish(value: StyleTagOperation, update = false) {
  operation.value = value;
  if (attempted.value === value.id) attempted.value = undefined;
  feedback.value = {
    pending: '正在核对是否保存成功。',
    uncertain: '暂未确认保存结果。可核对服务端当前标签。',
    confirmed: '风格标签已更新。',
    unchanged: '风格标签未变更。',
    rejected: '未能保存风格标签，请重新选择后再试。',
    accepted_current_state: '已保留服务端当前标签。',
  }[value.phase];
  if (update && (value.phase === 'confirmed' || value.phase === 'accepted_current_state')) {
    draft.value = undefined;
    emit('refreshed');
  }
}
async function readOperations() {
  if (readingOperations.value) return;
  const run = dialogRun;
  const epoch = props.epoch;
  readingOperations.value = true;
  try {
    const reply = await core.styleTagOperations(epoch);
    if (run !== dialogRun || epoch !== props.epoch || reply.epoch !== epoch) return;
    operationsKnown.value = true;
    const recovered = reply.value.find((value) => value.id === attempted.value);
    const active = reply.value.find(
      (value) => value.phase === 'pending' || value.phase === 'uncertain',
    );
    if (active || recovered) publish(active ?? recovered!, !!recovered);
    else if (attempted.value)
      feedback.value = '尚未找到本次保存的回执。请稍后查看保存状态，避免重复提交。';
    else if (unresolved.value) operation.value = undefined;
  } catch (issue) {
    if (run === dialogRun && epoch === props.epoch) {
      operationsKnown.value = false;
      report(issue);
    }
  } finally {
    if (run === dialogRun) readingOperations.value = false;
  }
}
async function act(kind: 'save' | 'reconcile' | 'accept') {
  if (submitting.value || readingOperations.value) return;
  const run = dialogRun;
  const epoch = props.epoch;
  const current = operation.value;
  const target = draft.value;
  const revision = props.revision;
  const observed = current?.observed;
  if (
    kind === 'save'
      ? !canSave.value || !target || !revision
      : !current || !unresolved.value || (kind === 'accept' && (!observed || !observedNames.value))
  )
    return;
  const id = kind === 'save' ? crypto.randomUUID() : current!.id;
  if (kind === 'save') attempted.value = id;
  submitting.value = true;
  feedback.value = '';
  try {
    const reply =
      kind === 'save'
        ? await core.saveStyleTag(epoch, id, revision!, target!.categoryId, target!.tagId)
        : kind === 'reconcile'
          ? await core.reconcileStyleTag(epoch, id)
          : await core.acceptCurrentStyleTag(epoch, id, observed!.revision);
    if (run !== dialogRun || epoch !== props.epoch || reply.epoch !== epoch) return;
    publish(reply.value, true);
  } catch (issue) {
    if (run !== dialogRun || epoch !== props.epoch) return;
    // A lost IPC reply is not permission to send another write.
    report(issue);
    if (kind === 'save') {
      const code = failure(issue).code;
      if (['request_not_sent', 'snapshot_changed', 'invalid_input'].includes(code)) {
        attempted.value = undefined;
        draft.value = undefined;
        if (code === 'snapshot_changed') emit('refreshed');
      } else {
        // These guards run before this new UUID is journaled. Recover any other
        // operation without leaving a local UUID that can never receive a receipt.
        if (code === 'busy' || code === 'write_uncertain') attempted.value = undefined;
        void readOperations();
      }
    }
  } finally {
    if (run === dialogRun) submitting.value = false;
  }
}

async function load() {
  const epoch = props.epoch;
  const current = ++request;
  loading.value = true;
  error.value = '';
  try {
    const result = await core.styleRecommendationCategories(epoch);
    if (current !== request || props.epoch !== epoch || result.epoch !== epoch) return;
    categories.value = result.value;
    selected.value =
      result.value.find((item) => item.id === props.applied?.id)?.id ?? result.value[0]?.id;
  } catch (issue) {
    if (current === request && props.epoch === epoch) error.value = failure(issue).message;
  } finally {
    if (current === request) loading.value = false;
  }
}
async function show() {
  dialogRun++;
  open.value = true;
  categories.value = [];
  selected.value = undefined;
  draft.value = undefined;
  operationsKnown.value = false;
  feedback.value = '';
  await nextTick();
  if (!open.value || !dialog.value) return;
  dialog.value.showModal();
  void load();
  void readOperations();
}
function close(force = false) {
  if (submitting.value && !force) return;
  dialogRun++;
  submitting.value = false;
  readingOperations.value = false;
  request++;
  dialog.value?.close();
  open.value = false;
  trigger.value?.focus({ preventScroll: true });
}
watch(
  () => props.epoch,
  () => {
    operation.value = undefined;
    attempted.value = undefined;
    if (open.value) close(true);
  },
);
watch(
  () => props.revision,
  () => (draft.value = undefined),
);
onUnmounted(() => {
  request++;
  dialogRun++;
});
</script>

<template>
  <button ref="trigger" type="button" class="text-button style-tags-entry" @click="show">
    浏览风格标签
  </button>
  <dialog
    v-if="open"
    ref="dialog"
    class="style-tag-browser"
    aria-label="风格标签"
    @cancel.prevent="close()"
  >
    <header>
      <h2>风格标签</h2>
      <button
        type="button"
        class="icon-button"
        aria-label="关闭风格标签"
        :disabled="submitting"
        @click="close()"
      >
        <Icon name="close" />
      </button>
    </header>
    <p class="muted">切换分类只浏览。选择标签并确认后，才会更新推荐偏好。</p>
    <div class="style-tag-body">
      <div v-if="applied" class="style-tag-applied" role="group" aria-label="本次推荐的风格标签">
        <strong>当前风格 · {{ applied.name }}</strong>
        <ul>
          <li v-for="tag in applied.tags" :key="tag.id">{{ tag.name }}</li>
        </ul>
      </div>
      <p v-else class="muted">本次推荐未返回可显示的风格标签。</p>
      <p v-if="loading" role="status">正在读取风格标签…</p>
      <div v-else-if="error">
        <p role="alert">{{ error }}</p>
        <button type="button" @click="load">重试</button>
      </div>
      <p v-else-if="!categories.length">暂无可用风格分类。</p>
      <template v-else>
        <nav aria-label="风格分类">
          <button
            v-for="item in categories"
            :key="item.id"
            type="button"
            :aria-pressed="selected === item.id"
            @click="selected = item.id"
          >
            {{ item.name }}
          </button>
        </nav>
        <ul v-if="category?.tags.length" aria-label="当前分类标签">
          <li
            v-for="tag in category.tags"
            :key="tag.id"
            :class="{ 'style-tag-current': appliedIds.has(tag.id) }"
          >
            <button
              type="button"
              :aria-pressed="draft?.categoryId === category.id && draft.tagId === tag.id"
              :disabled="submitting || unresolved || !!attempted"
              @click="draft = { categoryId: category.id, tagId: tag.id }"
            >
              {{ tag.name }}
            </button>
            <span v-if="appliedIds.has(tag.id)" class="style-tag-current-label">当前风格</span>
          </li>
        </ul>
        <p v-else>此分类暂无标签。</p>
      </template>
    </div>
    <p v-if="!applied" class="muted">暂时无法确认当前标签，请刷新推荐后再保存。</p>
    <p v-if="feedback" role="status">{{ feedback }}</p>
    <p v-if="unresolved && unresolvedLabel" class="muted style-tag-unresolved">
      待核对：{{ unresolvedLabel }}
    </p>
    <p v-if="unresolved && observedNames" class="muted">服务端当前标签：{{ observedNames }}</p>
    <p v-else-if="unresolved && operation?.observed" class="muted">
      当前标签名称不完整，暂不能确认保留。
    </p>
    <p v-if="draftLabel && !unresolved" class="style-tag-draft">准备更新为：{{ draftLabel }}</p>
    <footer>
      <button
        v-if="unresolved"
        type="button"
        :disabled="submitting || readingOperations"
        @click="act('reconcile')"
      >
        核对服务端标签
      </button>
      <button
        v-if="unresolved && observedNames"
        type="button"
        :disabled="submitting || readingOperations"
        @click="act('accept')"
      >
        保留服务端当前标签
      </button>
      <button
        v-if="(!operationsKnown || attempted) && !unresolved"
        type="button"
        :disabled="submitting || readingOperations"
        @click="readOperations"
      >
        查看保存状态
      </button>
      <button type="button" class="primary" :disabled="!canSave" @click="act('save')">
        确认更新风格标签
      </button>
    </footer>
  </dialog>
</template>

<style scoped>
.style-tag-browser {
  width: min(574px, calc(100vw - 32px));
  max-height: calc(100dvh - 32px);
  padding: 18px 22px;
  border: 1px solid var(--line);
  border-radius: 12px;
  color: var(--text);
  background: var(--surface-raised);
}
.style-tag-browser[open] {
  display: flex;
  flex-direction: column;
}
.style-tag-browser::backdrop {
  background: #0004;
}
.style-tag-browser header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex: none;
}
.style-tag-browser h2 {
  margin: 0;
  font-size: 19px;
}
.style-tag-body {
  overflow: auto;
  min-height: 0;
}
.style-tag-body nav,
.style-tag-body ul {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.style-tag-body ul {
  list-style: none;
  padding: 0;
}
.style-tag-body li {
  padding: 6px 12px;
  border-radius: 24px;
  background: var(--surface-alt);
  overflow-wrap: anywhere;
}
.style-tag-current-label {
  margin-inline-start: 6px;
  font-size: 12px;
  color: var(--accent);
}
.style-tag-body button[aria-pressed='true'] {
  color: var(--accent);
}
.style-tag-browser footer {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  justify-content: flex-end;
  margin-top: 12px;
}
</style>
