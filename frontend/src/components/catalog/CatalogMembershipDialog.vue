<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useId } from 'vue';
import { failure, type AccountView } from '../../services/contracts';
import type { CatalogResource, CatalogMembershipListener } from '../../services/catalog';
import { catalogMembershipObservation } from '../../services/catalogMembership';
import { useCore } from '../../services/core';
import { useCatalogMembership } from '../../composables/useCatalogMembership';
import Icon from '../Icon.vue';
const props = defineProps<{
  account: AccountView;
  resource: CatalogResource;
  name: string;
  desired: boolean;
  changed: CatalogMembershipListener;
}>();
const emit = defineEmits<{ close: []; expired: [] }>();
const core = useCore(),
  model = useCatalogMembership(core, () => emit('expired'));
const dialog = ref<HTMLDialogElement>(),
  loading = ref(false),
  error = ref('');
const observed = ref<boolean | null>(null);
const writing = ref(false);
const resource = { ...props.resource };
const artist = resource.kind === 'artist';
const titleId = useId();
const title = computed(() =>
  artist
    ? props.desired
      ? '关注歌手'
      : '取消关注歌手'
    : props.desired
      ? '收藏专辑'
      : '删除收藏专辑',
);
const owner = props.account.epoch;
const busy = computed(() => loading.value || model.busy.value);
const message = computed(
  () =>
    error.value ||
    model.error.value ||
    (model.operation.value?.phase === 'rejected' ? '服务器未接受本次操作，请重新读取后再试。' : ''),
);
const canWrite = computed(
  () =>
    !busy.value &&
    !model.pending.value &&
    model.known.value &&
    observed.value !== null &&
    observed.value !== props.desired,
);
let alive = true;
model.connect(props.account, resource);
onMounted(() => {
  dialog.value?.showModal();
  void load();
});
onBeforeUnmount(() => {
  alive = false;
  model.dispose();
});
async function load() {
  if (busy.value) return;
  const origin = document.activeElement;
  const fromDialog = !!origin && !!dialog.value?.contains(origin);
  loading.value = true;
  error.value = '';
  observed.value = null;
  try {
    const [result] = await Promise.all([
      catalogMembershipObservation(core, owner, resource),
      model.command({ kind: 'inspect' }),
    ]);
    if (!alive) return;
    observed.value = result;
    if (observed.value === null)
      error.value = artist ? '暂时无法确认关注状态，请重试' : '暂时无法确认收藏状态，请重试';
  } catch (issue) {
    if (!alive) return;
    const value = failure(issue);
    error.value = value.message;
    if (['session_expired', 'profile_changed'].includes(value.code)) emit('expired');
  } finally {
    if (alive) {
      loading.value = false;
      await nextTick();
      if (fromDialog && document.activeElement === document.body)
        (origin instanceof HTMLElement && origin.isConnected && !origin.matches(':disabled')
          ? origin
          : dialog.value?.querySelector<HTMLElement>('footer button:not(:disabled)')
        )?.focus({ preventScroll: true });
    }
  }
}
async function command(reconcile = false) {
  if (reconcile ? busy.value || !model.pending.value : !canWrite.value) return;
  writing.value = true;
  props.changed(owner, resource);
  const value = await model.command(
    reconcile ? { kind: 'reconcile' } : { kind: 'set', desired: props.desired },
  );
  props.changed(owner, resource);
  if (!alive) return;
  writing.value = false;
  // A write or uncertain journal invalidates old read-only collection snapshots.
  if (value !== null) {
    observed.value = value;
    if (value === props.desired) {
      emit('close');
      return;
    }
  }
  await nextTick();
  if (document.activeElement === document.body)
    dialog.value?.querySelector<HTMLButtonElement>('footer button:not(:disabled)')?.focus();
}
</script>
<template>
  <dialog
    ref="dialog"
    class="playlist-dialog catalog-membership-dialog"
    :class="artist ? 'artist-membership-dialog' : 'album-membership-dialog'"
    :aria-labelledby="titleId"
    :aria-busy="busy"
    @cancel.prevent="!writing && emit('close')"
  >
    <header>
      <h2 :id="titleId">{{ title }}</h2>
      <button
        class="icon-button"
        :aria-label="artist ? '关闭歌手关注确认' : '关闭专辑收藏确认'"
        :disabled="writing"
        @click="emit('close')"
      >
        <Icon name="close" />
      </button>
    </header>
    <p class="album-membership-name">{{ name }}</p>
    <p v-if="loading" role="status">
      {{ artist ? '正在核对当前关注状态…' : '正在核对当前收藏状态…' }}
    </p>
    <p v-else-if="model.pending.value">上次操作结果尚未确认，请先核对结果。</p>
    <p v-else-if="observed === desired">
      {{
        artist
          ? desired
            ? '你已经关注了这位歌手。'
            : '你已经取消关注这位歌手。'
          : desired
            ? '这张专辑已在你的收藏中。'
            : '这张专辑已不在你的收藏中。'
      }}
    </p>
    <p v-else-if="observed !== null">
      {{
        artist
          ? desired
            ? '关注这位歌手？'
            : '取消关注这位歌手？'
          : desired
            ? '将这张专辑加入你的收藏？'
            : '从你的收藏中删除这张专辑？已下载的文件会保留。'
      }}
    </p>
    <p v-if="message" class="catalog-error" role="alert">{{ message }}</p>
    <footer>
      <button class="secondary" autofocus :disabled="writing" @click="emit('close')">
        {{ observed === desired && !model.pending.value ? '完成' : '取消' }}
      </button>
      <button v-if="model.pending.value" class="primary" :disabled="busy" @click="command(true)">
        核对结果
      </button>
      <button
        v-else-if="observed === null || !model.known.value"
        class="primary"
        :disabled="busy"
        @click="load"
      >
        重新读取
      </button>
      <button
        v-else-if="observed !== desired"
        class="primary"
        :disabled="!canWrite"
        @click="command()"
      >
        {{ busy ? '正在处理…' : '确认' }}
      </button>
    </footer>
  </dialog>
</template>
