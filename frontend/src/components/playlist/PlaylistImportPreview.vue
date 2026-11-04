<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from 'vue';
import { useCore } from '../../services/core';
import {
  maxExternalImportTextBytes,
  type ExternalImportInput,
} from '../../services/externalPlaylistImport';
import type { ExternalPlaylistInput, ExternalPlaylistSource } from '../../services/playlists';
import Icon from '../Icon.vue';
import ModalTransition from '../ModalTransition.vue';
import PlaylistImportSubmit from './PlaylistImportSubmit.vue';

const props = defineProps<{ accountEpoch: string; authenticated: boolean }>();
const core = useCore();
const trigger = shallowRef<HTMLElement>();
const dialog = ref<HTMLDialogElement>();
const open = ref(false);
const title = ref('歌单导入预览');
const authorizationPanel = ref(false);
const tabs = [
  ['links', '链接导入'],
  ['text', '文字导入'],
] as const;
type ImportTab = (typeof tabs)[number][0];
const tab = ref<ImportTab>('links');
const inputs = ref(['']);
const rawText = ref('');
const observed = ref<ExternalPlaylistInput[]>();
const busy = ref(false);
const issue = ref('');
const maxLinks = 10;
let generation = 0;
const recognized = computed(
  () => observed.value?.filter((item) => item.kind === 'link').length ?? 0,
);
const canRead = computed(() => inputs.value.some((input) => input.trim().length > 0));
const textBytes = computed(() =>
  rawText.value.length > maxExternalImportTextBytes
    ? maxExternalImportTextBytes + 1
    : new TextEncoder().encode(rawText.value).length,
);
const textTooLarge = computed(() => textBytes.value > maxExternalImportTextBytes);
const input = computed<ExternalImportInput>(() =>
  tab.value === 'links'
    ? { kind: 'links', inputs: inputs.value }
    : { kind: 'text', text: rawText.value },
);
const ready = computed(() =>
  tab.value === 'links'
    ? recognized.value > 0 && !observed.value?.some((item) => item.kind === 'unrecognized')
    : textBytes.value > 0 && !textTooLarge.value,
);
const names: Record<ExternalPlaylistSource, string> = {
  qq: 'QQ音乐',
  kugou: '酷狗音乐',
  kuwo: '酷我音乐',
  xiaomi: '小米音乐',
};

function invalidate() {
  generation++;
  observed.value = undefined;
  issue.value = '';
  busy.value = false;
}
watch(inputs, invalidate, { deep: true, flush: 'sync' });
watch([() => props.accountEpoch, () => props.authenticated], close, { flush: 'sync' });
onBeforeUnmount(invalidate);

function sourceLabel(entry: ExternalPlaylistInput | undefined) {
  if (entry?.kind !== 'link') return '';
  return entry.source ? `${names[entry.source]} · ${entry.host}` : entry.host;
}

function focusLink(index: number) {
  return dialog.value?.querySelector<HTMLInputElement>(`[data-import-input="${index}"]`)?.focus();
}
async function show(anchor: HTMLElement, origin: 'sidebar' | 'library') {
  if (open.value || !props.authenticated) return;
  trigger.value = anchor;
  title.value = origin === 'sidebar' ? '歌单导入' : '歌单导入预览';
  inputs.value = [''];
  rawText.value = '';
  tab.value = 'links';
  authorizationPanel.value = false;
  open.value = true;
  await nextTick();
  dialog.value?.showModal();
  focusLink(0);
}
defineExpose({ show });
function close() {
  invalidate();
  inputs.value = [''];
  rawText.value = '';
  open.value = false;
}
function chooseTab(target: ImportTab) {
  if (tab.value === target) return;
  // The fixed H5 switches component instances; the abandoned form is discarded.
  inputs.value = [''];
  rawText.value = '';
  tab.value = target;
}
async function tabKey(event: KeyboardEvent) {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  chooseTab(
    event.key === 'Home'
      ? 'links'
      : event.key === 'End'
        ? 'text'
        : tab.value === 'links'
          ? 'text'
          : 'links',
  );
  await nextTick();
  dialog.value?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
}
async function clearText() {
  rawText.value = '';
  await nextTick();
  dialog.value?.querySelector<HTMLElement>('[data-import-text]')?.focus();
}
async function add() {
  if (inputs.value.length >= maxLinks) return;
  inputs.value.push('');
  await nextTick();
  focusLink(inputs.value.length - 1);
}
async function remove(index: number) {
  if (inputs.value.length <= 1) return;
  inputs.value.splice(index, 1);
  await nextTick();
  focusLink(Math.min(index, inputs.value.length - 1));
}
async function preview() {
  if (!open.value || tab.value !== 'links' || busy.value || !canRead.value) return;
  const ticket = ++generation;
  const current = () => open.value && ticket === generation;
  busy.value = true;
  issue.value = '';
  observed.value = undefined;
  try {
    const result = await core.previewExternalPlaylist([...inputs.value]);
    if (current()) observed.value = result;
  } catch {
    if (current()) issue.value = '暂时无法预览链接，请检查输入后重试。';
  } finally {
    if (current()) busy.value = false;
  }
}
</script>

<template>
  <ModalTransition :return-focus="() => trigger ?? null">
    <dialog
      v-if="open"
      ref="dialog"
      class="playlist-dialog external-playlist-preview"
      aria-labelledby="external-playlist-title"
      @cancel.prevent="close"
    >
      <header>
        <h2 id="external-playlist-title">{{ title }}</h2>
        <button class="icon-button" type="button" aria-label="关闭导入预览" @click="close">
          <Icon name="close" />
        </button>
      </header>
      <form :aria-busy="busy" @submit.prevent="preview">
        <div
          v-show="!authorizationPanel"
          class="external-import-tabs"
          role="tablist"
          aria-label="导入方式"
        >
          <button
            v-for="[key, label] in tabs"
            :id="`external-import-tab-${key}`"
            :key="key"
            role="tab"
            type="button"
            :aria-controls="`external-import-panel-${key}`"
            :aria-selected="key === tab"
            :tabindex="key === tab ? 0 : -1"
            @click="chooseTab(key)"
            @keydown="tabKey"
          >
            {{ label }}
          </button>
        </div>
        <section
          v-show="!authorizationPanel && tab === 'links'"
          id="external-import-panel-links"
          role="tabpanel"
          aria-labelledby="external-import-tab-links"
        >
          <p class="muted">粘贴外部歌单分享链接。预览只识别链接，不读取曲目或提交导入。</p>
          <fieldset>
            <legend>链接</legend>
            <div v-for="(_, index) in inputs" :key="index" class="import-link-row">
              <label>
                链接 {{ index + 1 }}
                <input
                  v-model="inputs[index]"
                  :data-import-input="index"
                  maxlength="4096"
                  inputmode="url"
                  autocomplete="off"
                  spellcheck="false"
                  placeholder="粘贴歌单链接或分享文字"
                />
              </label>
              <button
                v-if="inputs.length > 1"
                type="button"
                class="text-button"
                :aria-label="`删除链接 ${index + 1}`"
                @click="remove(index)"
              >
                删除
              </button>
              <p v-if="observed?.[index]?.kind === 'link'" class="import-link-result">
                {{ sourceLabel(observed[index]) }}
              </p>
              <p v-else-if="observed?.[index]?.kind === 'unrecognized'" class="import-link-result">
                未识别到网页链接
              </p>
            </div>
          </fieldset>
          <button
            type="button"
            class="text-button"
            :disabled="inputs.length >= maxLinks"
            @click="add"
          >
            添加链接
          </button>
          <p v-if="inputs.length >= maxLinks" class="muted">最多预览 10 个链接。</p>
          <p v-if="issue" role="alert">{{ issue }}</p>
          <p v-else-if="observed" role="status">已识别 {{ recognized }} 个链接。</p>
        </section>
        <section
          v-show="!authorizationPanel && tab === 'text'"
          id="external-import-panel-text"
          role="tabpanel"
          aria-labelledby="external-import-tab-text"
        >
          <p class="muted">复制歌曲文字，再将整段文字粘贴到下方。确认后交给网易云音乐识别。</p>
          <label>
            歌曲文字
            <textarea
              v-model="rawText"
              data-import-text
              rows="5"
              autocomplete="off"
              spellcheck="false"
              placeholder="粘贴到此处"
              aria-describedby="external-import-text-limit"
              :aria-invalid="textTooLarge || undefined"
            />
          </label>
          <p
            id="external-import-text-limit"
            :role="textTooLarge ? 'alert' : undefined"
            class="muted"
          >
            {{ textTooLarge ? '文字超过 16 KiB，请缩短后再导入。' : '最多 16 KiB。' }}
          </p>
          <button v-if="rawText" type="button" class="text-button" @click="clearText">
            清空文字
          </button>
        </section>
        <PlaylistImportSubmit
          :epoch="accountEpoch"
          :input="input"
          :ready="ready"
          @authorization-panel="authorizationPanel = $event"
          @clear-text="rawText = ''"
        />
        <footer>
          <button type="button" class="secondary" @click="close">关闭</button>
          <button
            v-show="!authorizationPanel && tab === 'links'"
            type="submit"
            class="primary"
            :disabled="!canRead || busy"
          >
            {{ busy ? '正在识别…' : '预览链接' }}
          </button>
        </footer>
      </form>
    </dialog>
  </ModalTransition>
</template>

<style scoped>
.external-playlist-preview {
  width: min(416px, calc(100vw - 32px));
  max-height: min(576px, calc(100dvh - 32px));
  scroll-padding-block: 65px 85px;
}
.external-import-tabs {
  display: flex;
  gap: 14px;
  margin-bottom: 12px;
  border-bottom: 1px solid var(--line);
}
.external-import-tabs button {
  padding: 6px 3px;
  border-bottom: 2px solid transparent;
  color: var(--text-secondary);
}
.external-import-tabs [aria-selected='true'] {
  border-color: var(--accent);
  color: var(--text);
  font-weight: 600;
}
.external-playlist-preview [role='tabpanel'] > p:first-child {
  margin: 0 0 12px;
}
.external-playlist-preview textarea {
  width: 100%;
  min-height: 100px;
  resize: vertical;
}
.import-link-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 4px 8px;
  align-items: end;
}
.import-link-row label {
  min-width: 0;
  margin-bottom: 0;
}
.import-link-row input {
  width: 100%;
}
.import-link-result {
  grid-column: 1 / -1;
  margin: 0;
  overflow-wrap: anywhere;
  color: var(--muted);
}
</style>
