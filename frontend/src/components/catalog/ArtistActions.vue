<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, shallowRef } from 'vue';
import { failure, type AccountView } from '../../services/contracts';
import type { CatalogMembershipListener } from '../../services/catalog';
import { artistLink, type ArtistCardRequest } from '../../services/artistCards';
import { useCore } from '../../services/core';
import CatalogMembershipDialog from './CatalogMembershipDialog.vue';
import ModalTransition from '../ModalTransition.vue';
import MenuTransition from '../MenuTransition.vue';
const props = defineProps<{ account: AccountView; changed: CatalogMembershipListener }>();
const emit = defineEmits<{ expired: [] }>();
const core = useCore(),
  request = shallowRef<ArtistCardRequest>();
const membership = ref(false),
  message = ref(''),
  failed = ref(false),
  busy = ref(false);
const feedback = ref<HTMLElement>();
let sequence = 0,
  timer: ReturnType<typeof setTimeout> | undefined;
function close() {
  sequence++;
  clearTimeout(timer);
  request.value = undefined;
  membership.value = false;
  message.value = '';
  failed.value = false;
  busy.value = false;
}
onBeforeUnmount(close);
function dismiss() {
  const anchor = request.value?.anchor;
  close();
  if (anchor?.isConnected) anchor.focus({ preventScroll: true });
}
async function show(value: ArtistCardRequest) {
  close();
  request.value = value;
  if (value.action === 'collect') membership.value = true;
  else await copy();
}
async function copy() {
  if (!request.value || busy.value || props.account.status !== 'authenticated') return;
  const ticket = sequence,
    anchor = request.value.anchor;
  const internal = !!feedback.value?.contains(document.activeElement);
  busy.value = true;
  clearTimeout(timer);
  try {
    const link = artistLink(request.value.artist.id);
    if (!link) throw { code: 'invalid', message: '歌手链接暂不可用' };
    await core.copyText(link);
    if (ticket !== sequence) return;
    failed.value = false;
    message.value = '链接已复制';
    timer = setTimeout(() => {
      if (!feedback.value?.contains(document.activeElement)) message.value = '';
    }, 3500);
  } catch (issue) {
    if (ticket !== sequence) return;
    failed.value = true;
    message.value = failure(issue).message;
  } finally {
    if (ticket === sequence) {
      busy.value = false;
      await nextTick();
      if (ticket === sequence && internal && document.activeElement === document.body)
        anchor?.focus({ preventScroll: true });
    }
  }
}
defineExpose({ show, close });
const fallback = () =>
  document.querySelector<HTMLElement>(
    '.search-results-page [role="tab"][aria-selected="true"],.collections-page h1,.catalog-heading h1',
  );
</script>
<template>
  <ModalTransition :return-focus="() => request?.anchor ?? null" :fallback-focus="fallback">
    <CatalogMembershipDialog
      v-if="membership && request"
      :account="account"
      :resource="{ kind: 'artist', id: request.artist.id }"
      :name="request.artist.name"
      :desired="true"
      :changed="changed"
      @close="membership = false"
      @expired="emit('expired')"
    />
  </ModalTransition>
  <MenuTransition
    ><div
      v-if="message"
      ref="feedback"
      class="track-action-feedback artist-action-feedback"
      :class="{ failed }"
      role="status"
    >
      {{ message
      }}<button v-if="failed" class="text-button" :aria-disabled="busy" @click="copy">
        {{ busy ? '正在处理…' : '重试' }}</button
      ><button class="text-button" @click="dismiss">关闭</button>
    </div></MenuTransition
  >
</template>
