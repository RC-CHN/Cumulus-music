<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import type { useServerQuickLogin } from '../../composables/useServerQuickLogin';

const props = defineProps<{ name: string; model: ReturnType<typeof useServerQuickLogin> }>();
const emit = defineEmits<{ consent: []; back: []; alternate: [] }>();
const { view, pending, cancelling, problem, canRead } = props.model;
const panel = ref<HTMLElement>();
const back = ref<HTMLButtonElement>();
const state = computed(() => {
  if (cancelling.value) return 'cancelling';
  if (pending.value) return pending.value;
  if (problem.value) return problem.value;
  return view.value?.phase ?? 'consent';
});
const message = computed(
  () =>
    ({
      consent: '',
      preparing: '正在重新核对该账号的快捷登录资格…',
      ready: '尚未取得登录回执。可以返回账号列表，或使用其他登录方式。',
      exchanging: '正在向服务提交快捷登录…',
      checking_identity: '正在核对登录身份…',
      reconciling: '正在核对登录状态…',
      identity_required: '已取得登录回执，身份尚未确认。可以继续核对。',
      result_unknown: '未能确认登录结果。可以返回或改用其他登录方式。',
      verification_required: '此次快捷登录需要额外验证。请改用其他登录方式。',
      rejected: '服务未通过这次快捷登录。可以返回或改用其他登录方式。',
      authenticated: '登录已确认，正在打开账号…',
      cancelled: '已退出本次快捷登录。',
      preflight: '暂时无法准备快捷登录。请返回并重新核对资格，或改用其他登录方式。',
      read_required: '尚未确认登录结果。请先核对登录状态。',
      cancelling: '正在退出本次快捷登录…',
    })[state.value],
);
const alternate = computed(() =>
  [
    'ready',
    'result_unknown',
    'verification_required',
    'rejected',
    'preflight',
    'cancelled',
  ].includes(state.value),
);
watch(state, async () => {
  await nextTick();
  const root = panel.value;
  const active = document.activeElement;
  if (!root?.isConnected || !root.closest('dialog[open]')) return;
  if (
    active === document.body ||
    !active?.isConnected ||
    (root.contains(active) && active.matches(':disabled'))
  ) {
    const target = back.value && !back.value.disabled ? back.value : root;
    target.focus({ preventScroll: true });
    target.scrollIntoView({ block: 'nearest' });
  }
});
</script>

<template>
  <section
    ref="panel"
    class="account-server-quick"
    tabindex="-1"
    aria-label="快捷登录进度"
    data-testid="server-quick-login-panel"
    :data-server-quick-phase="state"
    :aria-busy="!!pending || cancelling"
  >
    <p class="account-server-quick-target">{{ name }}</p>
    <template v-if="state === 'consent'">
      <p id="server-quick-login-consent-note">
        将向网易云音乐服务提交最多 3 个本机保存账号的登录资料，尝试登录所选账号。
      </p>
    </template>
    <p v-else :role="problem ? 'alert' : 'status'">{{ message }}</p>
    <div class="account-confirm account-server-quick-actions">
      <button
        type="button"
        class="secondary"
        ref="back"
        data-confirmation-cancel
        data-testid="server-quick-login-cancel"
        :disabled="cancelling || state === 'authenticated'"
        @click="emit('back')"
      >
        {{ state === 'consent' ? '取消' : pending ? '取消登录' : '返回账号列表' }}
      </button>
      <button
        v-if="state === 'consent'"
        type="button"
        class="primary"
        data-testid="server-quick-login-consent"
        aria-describedby="server-quick-login-consent-note"
        @click="emit('consent')"
      >
        同意并快捷登录
      </button>
      <button
        v-else-if="canRead && !pending"
        type="button"
        class="primary"
        data-testid="server-quick-login-reconcile"
        :disabled="cancelling"
        @click="model.reconcile"
      >
        核对登录状态
      </button>
      <button
        v-else-if="alternate"
        type="button"
        class="primary"
        data-testid="server-quick-login-alternate"
        @click="emit('alternate')"
      >
        其他登录方式
      </button>
    </div>
  </section>
</template>
