<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { LoginModel } from '../../composables/useLogin';
import Icon from '../Icon.vue';
const props = defineProps<{ model: LoginModel }>();
const emit = defineEmits<{ back: [] }>();
const question = computed(() => props.model.view.value?.question);
const challenge = computed(() => props.model.view.value?.challenge);
const attempted = ref(false);
const phase = computed(() => {
  const value = question.value?.phase;
  if (!value) return attempted.value && !props.model.busy.value ? 'failed' : 'loading';
  if (props.model.busy.value && value === 'failed') return 'loading';
  return props.model.busy.value && value === 'ready' ? 'verifying' : value;
});
const root = ref<HTMLElement>();
const answer = ref(''),
  edited = ref(false);
let alive = true;
function clearSecrets() {
  answer.value = '';
  for (const input of root.value?.querySelectorAll<HTMLInputElement>('input') ?? [])
    input.checked = false;
}
defineExpose({ clearSecrets });
async function focusStep() {
  await nextTick();
  if (!alive || document.activeElement !== root.value || props.model.busy.value) return;
  const target =
    phase.value === 'ready'
      ? (root.value?.querySelector<HTMLElement>('input:checked') ??
        root.value?.querySelector<HTMLElement>('input'))
      : root.value?.querySelector<HTMLElement>('.question-failed button');
  target?.focus({ preventScroll: true });
}
async function load() {
  if (!alive || props.model.busy.value) return;
  clearSecrets();
  attempted.value = true;
  edited.value = false;
  root.value?.focus({ preventScroll: true });
  await props.model.run({ kind: 'question_read' });
  await focusStep();
}
async function submit() {
  const q = question.value;
  if (
    !alive ||
    !q ||
    q.phase !== 'ready' ||
    props.model.busy.value ||
    !answer.value ||
    root.value?.closest('[inert]')
  )
    return;
  edited.value = false;
  root.value?.focus({ preventScroll: true });
  await props.model.run({ kind: 'question_answer', question_id: q.id, answer: answer.value });
  await focusStep();
}
async function reconcile() {
  if (!alive || props.model.busy.value || phase.value !== 'recovery_required') return;
  root.value?.focus({ preventScroll: true });
  await props.model.run({ kind: 'reconcile' });
  await focusStep();
}
watch(() => question.value?.id, clearSecrets);
watch(phase, (value) => {
  if (value === 'recovery_required') clearSecrets();
});
onMounted(() => {
  if (!question.value) void load();
});
onBeforeUnmount(() => {
  alive = false;
  clearSecrets();
});
</script>
<template>
  <section
    ref="root"
    class="question-verification"
    tabindex="-1"
    aria-label="账号安全问题"
    :data-question-phase="phase"
    :aria-busy="model.busy.value"
  >
    <button type="button" class="question-back" aria-label="返回登录" @click="emit('back')">
      <Icon name="back" />
    </button>
    <div v-if="phase === 'loading'" class="question-loading" role="status">
      <span class="device-spinner" aria-hidden="true" /><span class="question-loading-text"
        >正在读取账号问题…</span
      >
    </div>
    <div v-else-if="phase === 'failed'" class="question-failed" role="alert">
      <p>暂时无法读取账号问题，请重试。</p>
      <button type="button" class="primary" :disabled="model.busy.value" @click="load">
        重试读取
      </button>
    </div>
    <div v-else-if="phase === 'recovery_required'" class="question-failed" role="status">
      <p>无法确认上次验证结果，请先核对登录状态。</p>
      <button type="button" class="primary" :disabled="model.busy.value" @click="reconcile">
        {{ model.busy.value ? '正在核对…' : '核对登录状态' }}
      </button>
      <p v-if="model.error.value" class="verification-error" role="alert">
        {{ model.error.value }}
      </p>
    </div>
    <form v-else class="login-questions" @submit.prevent="submit">
      <p class="question-description">{{ challenge?.question }}</p>
      <div
        class="question-options"
        role="radiogroup"
        :aria-label="challenge?.question ?? '账号问题选项'"
        :data-question-type="question?.questionType ?? 'unknown'"
      >
        <label
          v-for="item in challenge?.answers"
          :key="item.value"
          class="question-option"
          :class="{
            'question-device': question?.questionType === 1 && item.value !== 'none',
            'question-none': item.value === 'none',
            'question-artist': question?.questionType === 3 && item.value !== 'none',
          }"
        >
          <input
            v-model="answer"
            type="radio"
            name="login-answer"
            :value="item.value"
            :disabled="model.busy.value || phase !== 'ready'"
            @click="edited = true"
          />
          <span class="question-picture" aria-hidden="true">
            <Icon
              :name="
                item.value === 'none'
                  ? 'help'
                  : question?.questionType === 1
                    ? 'monitor'
                    : question?.questionType === 3
                      ? 'user'
                      : 'album'
              "
            />
            <img
              v-if="item.picture"
              :src="item.picture"
              alt=""
              referrerpolicy="no-referrer"
              @error="($event.target as HTMLImageElement).hidden = true"
            />
            <img
              v-if="item.tag && question?.questionType === 3"
              class="question-tag"
              :src="item.tag"
              alt=""
              referrerpolicy="no-referrer"
              @error="($event.target as HTMLImageElement).hidden = true"
            />
          </span>
          <span class="question-info"
            ><span class="question-name" :title="item.title">{{ item.title }}</span
            ><small v-if="item.subtitle && question?.questionType !== 1" :title="item.subtitle">{{
              item.subtitle
            }}</small></span
          >
          <span class="question-check" aria-hidden="true" />
        </label>
      </div>
      <button
        type="submit"
        class="primary question-next"
        :disabled="model.busy.value || !answer || phase !== 'ready'"
      >
        {{ model.busy.value ? '正在验证…' : '下一步' }}
      </button>
      <p v-if="question?.answerRejected && !edited" class="verification-error" role="alert">
        {{ question.code === 509 ? '验证过于频繁，请稍后再试' : '答案验证未通过，请重新选择' }}
      </p>
      <p v-else-if="model.error.value && !edited" class="verification-error" role="alert">
        {{ model.error.value }}
      </p>
    </form>
  </section>
</template>
