<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { LoginNoticeView } from '../../services/auth';
import { useControlFocus } from '../../composables/useControlFocus';
import { useCore } from '../../services/core';
import Icon from '../Icon.vue';
const props = defineProps<{ notice: LoginNoticeView }>();
const emit = defineEmits<{ phone: []; other: [] }>();
const core = useCore();
const root = ref<HTMLElement>();
const heading = ref<HTMLElement>();
const opening = ref(false),
  documentError = ref('');
let alive = true;
const documentFocus = useControlFocus(
  () => (alive ? props.notice : null),
  () => root.value?.querySelector<HTMLElement>('.login-notice-release'),
);
async function focusNotice() {
  await nextTick();
  const panel = root.value;
  if (!alive || !panel?.isConnected || panel.closest('[inert]')) return;
  const focused = document.activeElement;
  // A persistent dialog control retains focus when the user chose it.
  if (
    focused !== document.body &&
    focused !== panel.closest('.login-flow') &&
    !panel.closest('.login-flow')?.contains(focused)
  )
    return;
  heading.value?.focus({ preventScroll: true });
  panel.scrollIntoView({ block: 'nearest' });
}
async function openRelease() {
  if (!alive || opening.value) return;
  const restoreFocus = documentFocus.capture(document.activeElement);
  opening.value = true;
  documentError.value = '';
  try {
    await core.openLoginDocument('client_release');
  } catch {
    if (alive) documentError.value = '浏览器未能打开，请检查默认浏览器后重试';
  } finally {
    if (alive) opening.value = false;
    await nextTick();
    restoreFocus();
  }
}
watch(
  () => props.notice.kind,
  () => {
    documentError.value = '';
    void focusNotice();
  },
);
onMounted(() => void focusNotice());
onBeforeUnmount(() => {
  alive = false;
});
</script>
<template>
  <section
    ref="root"
    class="login-notice"
    :data-notice-kind="notice.kind"
    aria-labelledby="login-notice-title"
  >
    <button
      type="button"
      class="login-notice-back"
      aria-label="返回手机登录"
      @click="emit('phone')"
    >
      <Icon name="back" />
    </button>
    <svg
      v-if="notice.kind === 'risk'"
      class="login-notice-illustration"
      viewBox="0 0 80 90"
      fill="none"
      aria-hidden="true"
    >
      <path
        d="M40 5 70 17v24c0 20-12 34-30 43C22 75 10 61 10 41V17L40 5Z"
        fill="var(--warning-bg)"
        stroke="currentColor"
        stroke-width="2"
      />
      <path d="M40 26v24" stroke="currentColor" stroke-width="5" stroke-linecap="round" />
      <circle cx="40" cy="62" r="3" fill="currentColor" />
    </svg>
    <Icon
      v-else
      class="login-notice-symbol"
      :name="notice.kind === 'upgrade' ? 'download' : 'lock'"
    />
    <h3 id="login-notice-title" ref="heading" class="login-notice-title" tabindex="-1">
      {{ notice.title }}
    </h3>
    <div
      v-if="notice.paragraphs.length || notice.kind !== 'disabled'"
      class="login-notice-content"
      role="region"
      aria-labelledby="login-notice-title"
      tabindex="0"
    >
      <p v-for="(paragraph, index) in notice.paragraphs" :key="index">{{ paragraph }}</p>
      <p v-if="!notice.paragraphs.length && notice.kind === 'risk'">暂无可显示的详细说明。</p>
      <p v-if="!notice.paragraphs.length && notice.kind === 'upgrade'">服务端要求更新登录组件。</p>
    </div>
    <template v-if="notice.kind === 'upgrade'">
      <p class="login-notice-community">
        Cumulus 是社区客户端。可以查看社区版本更新，是否能满足服务端登录要求尚未确认。
      </p>
      <button
        type="button"
        class="primary login-notice-release"
        :disabled="opening"
        @click="openRelease"
      >
        {{ opening ? '正在打开…' : '查看社区版本' }}
      </button>
      <p v-if="documentError" class="verification-error" role="alert">{{ documentError }}</p>
      <button type="button" class="login-notice-other" @click="emit('other')">
        尝试其他登录方式
      </button>
    </template>
  </section>
</template>
