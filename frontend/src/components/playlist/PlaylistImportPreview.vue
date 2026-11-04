<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useCore } from '../../services/core';
import type { ExternalPlaylistInput, ExternalPlaylistSource } from '../../services/playlists';
import Icon from '../Icon.vue';
import ModalTransition from '../ModalTransition.vue';

const props = defineProps<{ accountEpoch: string }>();
const core = useCore();
const trigger = ref<HTMLButtonElement>();
const dialog = ref<HTMLDialogElement>();
const open = ref(false);
const inputs = ref(['']);
const observed = ref<ExternalPlaylistInput[]>();
const busy = ref(false);
const issue = ref('');
const maxLinks = 10;
let generation = 0;
const recognized = computed(
  () => observed.value?.filter((item) => item.kind === 'link').length ?? 0,
);
const canRead = computed(() => inputs.value.some((input) => input.trim().length > 0));
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
watch(() => props.accountEpoch, close, { flush: 'sync' });
onBeforeUnmount(invalidate);

function sourceLabel(entry: ExternalPlaylistInput | undefined) {
  if (entry?.kind !== 'link') return '';
  return entry.source ? `${names[entry.source]} · ${entry.host}` : entry.host;
}

function focusLink(index: number) {
  return dialog.value?.querySelector<HTMLInputElement>(`[data-import-input="${index}"]`)?.focus();
}
async function show() {
  if (open.value) return;
  inputs.value = [''];
  open.value = true;
  await nextTick();
  dialog.value?.showModal();
  focusLink(0);
}
function close() {
  invalidate();
  inputs.value = [''];
  open.value = false;
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
  if (!open.value || busy.value || !canRead.value) return;
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
  <button ref="trigger" class="text-button" aria-haspopup="dialog" @click="show">
    歌单导入预览
  </button>
  <ModalTransition :return-focus="() => trigger ?? null">
    <dialog
      v-if="open"
      ref="dialog"
      class="playlist-dialog external-playlist-preview"
      aria-labelledby="external-playlist-title"
      @cancel.prevent="close"
    >
      <header>
        <h2 id="external-playlist-title">歌单导入预览</h2>
        <button class="icon-button" type="button" aria-label="关闭导入预览" @click="close">
          <Icon name="close" />
        </button>
      </header>
      <form :aria-busy="busy" @submit.prevent="preview">
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
        <footer>
          <button type="button" class="secondary" @click="close">关闭</button>
          <button type="submit" class="primary" :disabled="!canRead || busy">
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
}
.external-playlist-preview form > p:first-child {
  margin: 0 0 16px;
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
