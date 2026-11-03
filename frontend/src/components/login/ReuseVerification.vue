<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { LoginModel } from '../../composables/useLogin';
import { useNicknameDraft } from '../../composables/useNicknameDraft';
import type { LoginCommand } from '../../services/auth';
import { nicknameError, reuseDate, reuseTitle } from '../../services/phoneReuse';
import { nicknameFormatIssue } from '../../services/nickname';
import Icon from '../Icon.vue';
import NicknameEditor from './NicknameEditor.vue';
const props = defineProps<{ model: LoginModel }>();
const emit = defineEmits<{ back: []; phone: []; title: [value: string | null] }>();
const reuse = computed(() => props.model.view.value?.reuse);
const smsInvalid = computed(() => reuse.value?.nicknameError === 'sms_invalid');
const root = ref<HTMLElement>();
const attempted = ref(false);
const nicknameDraft = useNicknameDraft(
  () => {
    const current = reuse.value;
    const view = props.model.view.value;
    if (
      !view ||
      !current ||
      current.nicknameError === 'sms_invalid' ||
      !['nickname', 'hinting', 'checking_nickname', 'registering'].includes(current.phase)
    )
      return null;
    return {
      owner: JSON.stringify([view.operationId, current.id]),
      suggestion: current.suggestedNickname,
    };
  },
  () => {
    for (const input of root.value?.querySelectorAll<HTMLInputElement>('[aria-label="注册昵称"]') ??
      [])
      input.value = '';
  },
);
const { name, edited, recommendationError } = nicknameDraft;
const nameUnavailable = computed(() => reuse.value?.nicknameError === 'taken' && !edited.value);
const action = ref<'loading' | 'confirming' | 'hinting' | 'checking_nickname' | null>(null);
const phase = computed(
  () =>
    action.value ??
    reuse.value?.phase ??
    (attempted.value && !props.model.busy.value ? 'read_failed' : 'loading'),
);
const group = computed(() =>
  ['login_consent', 'confirming'].includes(phase.value)
    ? 'login_consent'
    : ['hinting', 'checking_nickname', 'registering'].includes(phase.value)
      ? 'nickname'
      : phase.value,
);
let alive = true;
watch(phase, (value) => emit('title', reuseTitle(value)), { immediate: true });
function clearSecrets() {
  nicknameDraft.clear();
  for (const input of root.value?.querySelectorAll('input') ?? []) input.value = '';
}
defineExpose({ clearSecrets });
function leaving(element: Element) {
  (element as HTMLElement).inert = true;
  for (const input of element.querySelectorAll<HTMLInputElement>('input')) input.value = '';
}
async function focusStep() {
  await nextTick();
  if (!alive || props.model.busy.value || document.activeElement !== root.value) return;
  Array.from(
    root.value?.querySelectorAll<HTMLElement>(
      'input:not(:disabled),button:not(:disabled):not(.reuse-back)',
    ) ?? [],
  )
    .find((element) => !element.closest('[inert]'))
    ?.focus({ preventScroll: true });
}
async function run(command: LoginCommand, pending: typeof action.value = null) {
  if (!alive || props.model.busy.value || action.value || root.value?.closest('[inert]')) return;
  root.value?.focus({ preventScroll: true });
  action.value = pending;
  try {
    return await props.model.run(command);
  } finally {
    action.value = null;
    await focusStep();
  }
}
async function load() {
  attempted.value = true;
  clearSecrets();
  await run({ kind: 'reuse_read' }, 'loading');
}
async function choose(choice: 'login' | 'register') {
  const id = reuse.value?.id;
  if (id) await run({ kind: 'reuse_choose', verification_id: id, choice });
}
async function back() {
  clearSecrets();
  const a = reuse.value;
  if (
    props.model.busy.value ||
    !a ||
    ['read_failed', 'recovery_required', 'choice'].includes(phase.value)
  ) {
    emit('back');
    return;
  }
  await run({ kind: 'reuse_back', verification_id: a.id }, 'loading');
}
async function confirm() {
  const id = reuse.value?.id;
  if (id) await run({ kind: 'reuse_confirm', verification_id: id }, 'confirming');
}
async function hint(prepare = false) {
  const id = reuse.value?.id;
  if (!id || props.model.busy.value || action.value || smsInvalid.value) return;
  const operation = props.model.view.value?.operationId;
  const request = nicknameDraft.recommendation();
  await run(
    { kind: prepare ? 'reuse_prepare_register' : 'reuse_nickname_hint', verification_id: id },
    'hinting',
  );
  if (
    alive &&
    props.model.view.value?.operationId === operation &&
    reuse.value?.id === id &&
    reuse.value.phase === 'nickname'
  )
    nicknameDraft.acceptRecommendation(
      request ?? (prepare ? nicknameDraft.recommendation() : null),
      reuse.value.suggestedNickname,
    );
}
async function register(useDefault = false) {
  const a = reuse.value;
  if (
    !a ||
    phase.value !== 'nickname' ||
    props.model.busy.value ||
    smsInvalid.value ||
    (useDefault ? !a.canSkipNickname : nameUnavailable.value || !!nicknameFormatIssue(name.value))
  )
    return;
  const nickname = useDefault ? null : name.value;
  const previous = reuse.value;
  nicknameDraft.submitted();
  const next = await run(
    { kind: 'reuse_register', verification_id: a.id, nickname },
    'checking_nickname',
  );
  if (alive && !next && props.model.error.value && reuse.value === previous) nicknameDraft.clear();
}
onMounted(() => {
  if (!reuse.value) void load();
});
onBeforeUnmount(() => {
  alive = false;
  clearSecrets();
  emit('title', null);
});
</script>
<template>
  <section
    ref="root"
    class="reuse-verification"
    tabindex="-1"
    aria-label="手机号关联账号确认"
    :data-reuse-phase="phase"
    :aria-busy="model.busy.value"
  >
    <button type="button" class="reuse-back" aria-label="返回登录" @click="emit('back')">
      <Icon name="back" />
    </button>
    <Transition name="reuse-step" mode="out-in" @before-leave="leaving" @after-enter="focusStep">
      <div v-if="phase === 'loading'" key="loading" class="reuse-loading" role="status">
        <span class="device-spinner" aria-hidden="true" /><span>正在读取关联账号…</span>
      </div>
      <div v-else-if="phase === 'choice'" key="choice" class="reuse-choice">
        <p class="reuse-description">
          当前手机号可能是运营商二次售卖号，<br />且已绑定以下账号，请确认是否是你本人
        </p>
        <div class="reuse-user">
          <span class="reuse-avatar" aria-hidden="true"
            ><Icon name="user" /><img
              v-if="model.view.value?.challenge.avatarUrl"
              :src="model.view.value.challenge.avatarUrl"
              alt=""
              referrerpolicy="no-referrer"
              @error="($event.target as HTMLImageElement).hidden = true"
          /></span>
          <div>
            <p>昵称：{{ reuse?.maskedNickname || '未知昵称' }}</p>
            <small v-if="reuseDate(reuse?.createdAt)"
              >注册时间：{{ reuseDate(reuse?.createdAt) }}</small
            >
          </div>
        </div>
        <div class="reuse-options">
          <button :disabled="model.busy.value" @click="choose('login')">
            是本人账号，立即登录<Icon name="forward" /></button
          ><button :disabled="model.busy.value" @click="choose('register')">
            非本人账号，重新注册<Icon name="forward" />
          </button>
        </div>
      </div>
      <div
        v-else-if="group === 'login_consent' || phase === 'register_consent'"
        :key="group"
        class="reuse-consent"
      >
        <div class="reuse-consent-content">
          <Icon name="info" class="reuse-info" />
          <p v-if="group === 'login_consent'">
            如果误登他人账号，可能导致账号被回收<br />点击登录授权运营商获取手机号进行状态验证
          </p>
          <p v-else>注册新账号后，该手机号将与原账号解绑</p>
        </div>
        <p
          v-if="phase === 'register_consent' && reuse?.registrationBlocked"
          class="reuse-boundary"
          role="status"
        >
          {{
            reuse.registrationBlocked === 'fresh_sms_required'
              ? '重新注册需要先用此手机号完成一次短信验证码登录。'
              : '上次注册结果尚未确认，请先重新短信登录核对，避免重复解绑。'
          }}
        </p>
        <button
          v-if="phase === 'register_consent' && reuse?.registrationBlocked"
          class="primary reuse-primary"
          :disabled="model.busy.value"
          @click="
            clearSecrets();
            emit('phone');
          "
        >
          重新短信登录
        </button>
        <button
          v-else
          class="primary reuse-primary"
          :disabled="model.busy.value || !!action"
          @click="group === 'login_consent' ? confirm() : hint(true)"
        >
          {{ group === 'login_consent' ? (model.busy.value ? '正在登录…' : '登录') : '注册' }}
        </button>
        <button class="reuse-cancel" :disabled="model.busy.value" @click="back">取消</button>
      </div>
      <div v-else-if="group === 'nickname'" key="nickname" class="reuse-nickname">
        <NicknameEditor
          :model-value="name"
          :disabled="model.busy.value || !!action || smsInvalid"
          :busy="model.busy.value || !!action"
          :submit-blocked="smsInvalid"
          :name-unavailable="nameUnavailable"
          :error="
            recommendationError ||
            (reuse?.nicknameError && (!edited || smsInvalid)
              ? nicknameError(reuse.nicknameError)
              : '')
          "
          :error-muted="reuse?.nicknameError === 'phone_registered' && !edited"
          :allow-default="!!reuse?.canSkipNickname && !smsInvalid"
          @update:model-value="nicknameDraft.edit"
          @random="hint()"
          @submit="register"
        >
          <p class="reuse-boundary">完成注册后，该手机号将与原账号解绑</p>
          <button
            v-if="smsInvalid"
            type="button"
            class="reuse-default"
            :disabled="model.busy.value"
            @click="
              clearSecrets();
              emit('phone');
            "
          >
            重新短信登录
          </button>
          <button
            type="button"
            class="reuse-cancel"
            :disabled="model.busy.value || !!action"
            @click="back"
          >
            取消
          </button>
        </NicknameEditor>
      </div>
      <div
        v-else
        class="reuse-failed"
        :key="phase"
        :role="phase === 'recovery_required' ? 'status' : 'alert'"
      >
        <Icon name="info" class="reuse-info" />
        <p>
          {{
            phase === 'read_failed'
              ? '操作失败，请稍后再试'
              : phase === 'login_failed'
                ? '登录未完成，请稍后再试'
                : '无法确认上次操作结果，请先核对登录状态。'
          }}
        </p>
        <button
          class="primary reuse-primary"
          :disabled="model.busy.value"
          @click="
            phase === 'read_failed'
              ? load()
              : phase === 'login_failed'
                ? confirm()
                : run({ kind: 'reconcile' })
          "
        >
          {{
            model.busy.value ? '正在核对…' : phase === 'recovery_required' ? '核对登录状态' : '重试'
          }}
        </button>
        <p v-if="model.error.value" class="verification-error" role="alert">
          {{ model.error.value }}
        </p>
        <button
          v-if="phase === 'recovery_required'"
          class="reuse-default"
          :disabled="model.busy.value"
          @click="
            clearSecrets();
            emit('phone');
          "
        >
          重新短信登录
        </button>
        <button
          v-else-if="phase === 'login_failed'"
          class="reuse-cancel"
          :disabled="model.busy.value"
          @click="back"
        >
          取消
        </button>
      </div>
    </Transition>
  </section>
</template>
