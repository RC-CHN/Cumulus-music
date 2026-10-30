<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import type { PlaylistManager } from '../../composables/usePlaylists';
import { usePlaylistEditor } from '../../composables/usePlaylistEditor';
import { playlistLength } from '../../services/playlists';
import Icon from '../Icon.vue';
import ActionMenu from '../ActionMenu.vue';
import ModalTransition from '../ModalTransition.vue';
import PlaylistCoverDialog from '../PlaylistCoverDialog.vue';
import PlaylistEditorConfirm from './PlaylistEditorConfirm.vue';
import PlaylistEditorTags from './PlaylistEditorTags.vue';
const props = defineProps<{
  manager: PlaylistManager;
  account: AccountView;
  target: { id: string; cover?: boolean };
}>();
const emit = defineEmits<{ close: []; coverChanged: []; expired: [] }>();
const { editor, busy, loading, error, tags, tagError, tagsLoading } = props.manager;
const form = usePlaylistEditor(
  props.manager,
  () => props.account,
  props.target.id,
  () => emit('close'),
  () => emit('coverChanged'),
);
const {
  name,
  description,
  selectedTags,
  loaded,
  message,
  confirmation,
  pending,
  outcome: result,
  fieldStatus,
  editable,
  invalid,
  dirty,
} = form;
const nameInput = ref<HTMLInputElement>(),
  coverOpen = ref(false);
const editorBody = ref<HTMLElement>(),
  tagSpace = ref(false);
async function tagVisibility(open: boolean) {
  tagSpace.value = open;
  if (!open) return;
  await nextTick();
  // Recovered Edit.jsx adds 250px padding and advances its scroll region.
  if (tagSpace.value && editorBody.value) editorBody.value.scrollTop += 250;
}
watch(loaded, async (ready) => {
  if (ready) {
    await nextTick();
    nameInput.value?.focus();
    if (props.target.cover && editable.value) coverOpen.value = true;
  }
});
const actions = computed(() => [
  {
    key: 'clear',
    label: '清空歌曲',
    icon: 'list',
    disabled:
      busy.value || pending.value || !editor.value?.trackIds.length || editor.value.hasLocal,
  },
  ...(editor.value?.private
    ? [{ key: 'publish', label: '设为公开', icon: 'user', disabled: busy.value || pending.value }]
    : []),
  { key: 'delete', label: '删除歌单', icon: 'trash', disabled: busy.value || pending.value },
]);
function action(key: string) {
  if (key === 'clear' || key === 'publish' || key === 'delete') confirmation.value = key;
}
defineExpose({ requestLeave: form.requestLeave });
</script>
<template>
  <section class="playlist-editor-page page-view" aria-labelledby="playlist-editor-heading">
    <h1 id="playlist-editor-heading">编辑歌单信息</h1>
    <div v-if="loading" class="empty-state" role="status">
      <span class="spinner" />
      <p>正在读取歌单…</p>
    </div>
    <div v-else-if="!editor" class="error-state" role="alert">
      <p>{{ error || '暂时无法读取歌单' }}</p>
      <button class="text-button" @click="manager.open(target.id)">重新读取歌单</button
      ><button class="text-button" aria-label="取消编辑歌单" @click="form.requestLeave()">
        返回
      </button>
    </div>
    <div v-else-if="!editable" class="empty-state">
      <p>此歌单不能编辑</p>
      <button class="download-button" aria-label="取消编辑歌单" @click="form.requestLeave()">
        返回
      </button>
    </div>
    <form v-else @submit.prevent="form.save">
      <div ref="editorBody" class="playlist-editor-body" :class="{ 'tag-dropdown-open': tagSpace }">
        <p v-if="error" class="error-state" role="alert">{{ error }}</p>
        <p v-if="message" class="notice" role="status">{{ message }}</p>
        <div v-if="result" class="mutation-result" :data-phase="result.phase" role="status">
          {{ result.message
          }}<span v-if="result.phase === 'partial'"
            >（名称 {{ fieldStatus.name }} / 标签 {{ fieldStatus.tags }} / 简介
            {{ fieldStatus.description }}）</span
          >
          <button
            v-if="pending"
            type="button"
            class="text-button"
            :disabled="busy"
            @click="form.reconcile"
          >
            核对结果
          </button>
        </div>
        <fieldset class="playlist-editor-grid" :disabled="busy || pending">
          <div class="playlist-editor-fields">
            <div class="playlist-editor-row">
              <label for="playlist-editor-name">名称：</label>
              <div class="playlist-editor-field">
                <input
                  id="playlist-editor-name"
                  ref="nameInput"
                  v-model="name"
                  aria-label="歌单名称"
                  aria-describedby="playlist-name-count"
                  :aria-invalid="
                    !name.trim() || /[@#]/.test(name) || playlistLength(name.trim()) > 40
                  "
                  placeholder="输入歌单标题"
                  maxlength="80"
                /><small
                  id="playlist-name-count"
                  :class="{ 'form-warning': playlistLength(name.trim()) > 40 }"
                  >{{ playlistLength(name.trim()) }} / 40 · 中文计 2 个字符</small
                >
              </div>
            </div>
            <div class="playlist-editor-row">
              <label for="playlist-editor-description">简介：</label>
              <div class="playlist-editor-field">
                <textarea
                  id="playlist-editor-description"
                  v-model="description"
                  aria-label="歌单简介"
                  aria-describedby="playlist-description-count"
                  :aria-invalid="playlistLength(description) > 1000"
                  maxlength="2000"
                /><small
                  id="playlist-description-count"
                  :class="{ 'form-warning': playlistLength(description) > 1000 }"
                  >{{ playlistLength(description) }} / 1000</small
                >
              </div>
            </div>
            <div class="playlist-editor-row">
              <span class="playlist-editor-label">标签：</span
              ><PlaylistEditorTags
                v-model="selectedTags"
                :tags="tags"
                :loading="tagsLoading"
                :error="tagError"
                :disabled="busy || pending"
                @retry="manager.loadTags"
                @visibility="tagVisibility"
              />
            </div>
            <p v-if="invalid" class="form-warning">
              名称不能为空、不能包含 @ 或 #，并请保持名称与简介在字数上限内。
            </p>
          </div>
          <div class="playlist-cover-edit">
            <button type="button" aria-label="替换歌单封面" @click="coverOpen = true">
              <img
                v-if="editor.coverUrl"
                :src="editor.coverUrl"
                alt="歌单封面"
                referrerpolicy="no-referrer"
              /><Icon v-else name="music" /><span><Icon name="edit" />替换图片</span>
            </button>
          </div>
        </fieldset>
      </div>
      <footer class="playlist-editor-actions">
        <button
          type="submit"
          class="primary"
          aria-label="保存修改"
          :disabled="busy || pending || invalid || !dirty"
        >
          {{ busy ? '正在保存…' : '保存' }}
        </button>
        <button
          type="button"
          class="download-button"
          aria-label="取消编辑歌单"
          :disabled="busy"
          @click="form.requestLeave()"
        >
          取消
        </button>
        <ActionMenu label="其他歌单操作" :items="actions" @select="action" />
        <span v-if="busy" class="muted" role="status">正在提交并核对…</span>
      </footer>
    </form>
    <Teleport to="body">
      <ModalTransition
        ><PlaylistEditorConfirm
          v-if="confirmation"
          :kind="confirmation"
          :name="editor?.name ?? ''"
          :count="editor?.trackIds.length ?? 0"
          :busy="busy"
          :pending="pending"
          @cancel="confirmation = undefined"
          @confirm="form.confirm"
      /></ModalTransition>
      <ModalTransition
        ><PlaylistCoverDialog
          v-if="coverOpen && editor"
          :account="account"
          :editor="editor"
          @close="
            coverOpen = false;
            manager.refreshOperations();
          "
          @saved="form.coverSaved"
          @expired="emit('expired')"
      /></ModalTransition>
    </Teleport>
  </section>
</template>
