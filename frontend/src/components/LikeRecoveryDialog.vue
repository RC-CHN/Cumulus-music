<script setup lang="ts">
import { nextTick, onMounted, ref } from 'vue';
import type { LikeRecoveryModel } from '../composables/useLikeRecovery';
import type { LikeOperation } from '../services/likeRecovery';
import Icon from './Icon.vue';
const props = defineProps<{ model: LikeRecoveryModel }>();
const dialog = ref<HTMLDialogElement>(),
  refresh = ref<HTMLButtonElement>(),
  cancel = ref<HTMLButtonElement>();
const confirmation = ref<LikeOperation>();
onMounted(() => dialog.value?.showModal());
async function read(offset?: number) {
  await props.model.refresh(offset);
  await nextTick();
  if (!dialog.value?.contains(document.activeElement)) refresh.value?.focus();
}
async function reconcile(row: LikeOperation) {
  await props.model.command({ kind: 'reconcile', id: row.id, revision: row.revision });
  await nextTick();
  if (!dialog.value?.contains(document.activeElement)) refresh.value?.focus();
}
async function ask(row: LikeOperation) {
  confirmation.value = { ...row };
  await nextTick();
  cancel.value?.focus();
}
async function back() {
  confirmation.value = undefined;
  await nextTick();
  refresh.value?.focus();
}
async function accept() {
  const row = confirmation.value;
  if (!row || row.observed === null) return;
  const ok = await props.model.command({
    kind: 'accept_current',
    id: row.id,
    revision: row.revision,
    observed: row.observed,
  });
  if (
    ok ||
    !props.model.view.value.operations.some((v) => v.id === row.id && v.revision === row.revision)
  )
    await back();
  else {
    await nextTick();
    if (!dialog.value?.contains(document.activeElement)) cancel.value?.focus();
  }
}
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog like-recovery-dialog"
    aria-labelledby="like-recovery-title"
    @cancel.prevent="!model.busy.value && (model.open.value = false)"
  >
    <header>
      <h2 id="like-recovery-title">核对喜欢状态</h2>
      <button
        class="icon-button"
        aria-label="关闭喜欢状态核对"
        :disabled="model.busy.value"
        @click="model.open.value = false"
      >
        <Icon name="close" />
      </button>
    </header>
    <p v-if="model.error.value" class="catalog-error" role="alert">{{ model.error.value }}</p>
    <p v-if="model.notice.value" role="status">{{ model.notice.value }}</p>
    <template v-if="confirmation">
      <h3>保留歌曲 {{ confirmation.id }} 的当前状态？</h3>
      <p>
        当前{{
          confirmation.observed ? '已喜欢' : '未喜欢'
        }}。这将结束本机对此次操作的等待，不会再次发送喜欢或取消请求。
      </p>
      <p class="muted">原请求仍可能稍后生效；如果其他设备上的状态发生变化，请重新核对。</p>
      <footer>
        <button ref="cancel" class="secondary" :disabled="model.busy.value" @click="back">
          取消</button
        ><button class="primary" :disabled="model.busy.value" @click="accept">
          确认保留当前状态
        </button>
      </footer>
    </template>
    <template v-else>
      <p class="muted">下列操作尚未确认。核对只读取喜欢列表，不会重复提交请求。</p>
      <button ref="refresh" class="text-button" :disabled="model.busy.value" @click="read()">
        刷新待核对记录
      </button>
      <p
        v-if="!model.view.value.total && !model.busy.value"
        class="like-recovery-empty"
        role="status"
      >
        没有待核对的喜欢操作。
      </p>
      <ul class="like-recovery-list" :aria-busy="model.busy.value">
        <li v-for="row in model.view.value.operations" :key="row.id" :data-song-id="row.id">
          <div>
            <strong>歌曲 {{ row.id }}</strong>
            <p>
              原操作：{{ row.desired ? '喜欢' : '取消喜欢' }} · 当前：{{
                row.observed === null ? '尚未核对' : row.observed ? '已喜欢' : '未喜欢'
              }}
            </p>
          </div>
          <div class="like-recovery-actions">
            <button class="secondary" :disabled="model.busy.value" @click="reconcile(row)">
              核对状态</button
            ><button
              v-if="row.canAccept"
              class="text-button"
              :disabled="model.busy.value"
              @click="ask(row)"
            >
              保留当前状态
            </button>
          </div>
        </li>
      </ul>
      <footer v-if="model.view.value.total > 20">
        <button
          class="secondary"
          :disabled="model.busy.value || !model.view.value.offset"
          @click="read(model.view.value.offset - 20)"
        >
          上一页</button
        ><span
          >{{ Math.floor(model.view.value.offset / 20) + 1 }} /
          {{ Math.ceil(model.view.value.total / 20) }}</span
        ><button
          class="secondary"
          :disabled="model.busy.value || model.view.value.offset + 20 >= model.view.value.total"
          @click="read(model.view.value.offset + 20)"
        >
          下一页
        </button>
      </footer>
    </template>
  </dialog>
</template>
<style scoped>
.like-recovery-dialog {
  width: min(620px, calc(100vw - 40px));
  line-height: 1.7;
}
.like-recovery-dialog .secondary {
  border: 1px solid var(--line-strong);
  border-radius: 18px;
  padding: 7px 16px;
  min-height: 34px;
}
.like-recovery-dialog .secondary:hover:not(:disabled) {
  background: var(--surface-hover);
}
.like-recovery-dialog p {
  margin: 12px 0;
  overflow-wrap: anywhere;
}
.like-recovery-list {
  list-style: none;
  padding: 0;
  margin: 0;
}
.like-recovery-list li {
  padding: 14px 0;
  border-top: 1px solid var(--line);
}
.like-recovery-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.like-recovery-dialog footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 12px;
  flex-wrap: wrap;
}
.like-recovery-empty {
  color: var(--muted);
}
</style>
