<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue';
import { failure } from '../services/contracts';
import { useCore } from '../services/core';
import type { EffectPeqPhase, EffectPeqView } from '../services/effectDirectory';

const props = defineProps<{ epoch: string; effectId: number; allowed: boolean }>();
const emit = defineEmits<{ changed: [] }>();
const core = useCore();
const group = ref<HTMLElement>();
const view = shallowRef<EffectPeqView>();
const busy = ref(false);
const error = ref('');
const confirmation = ref<string>();
let alive = true;
let observation = 0;
let observationTimer: ReturnType<typeof setTimeout> | undefined;
let observationRead: Promise<EffectPeqView> | undefined;

const labels: Record<EffectPeqPhase, string> = {
  idle: '可检查是否支持独立参数均衡',
  preparing: '材料尚未准备完成，可继续检查',
  prepared: '已验证为独立参数均衡组件，可启用',
  unsupported: '此音效不支持作为独立参数均衡组件启用',
  configured: '播放器已确认配置独立参数均衡',
  deferred: '等待音频加载后应用',
  recovery_required: '此音效有待处理的登记或本地状态',
};
const another = computed(() => !!view.value?.activeId && view.value.activeId !== props.effectId);
const mayPrepare = computed(
  () =>
    props.allowed &&
    !another.value &&
    !view.value?.release &&
    (view.value?.phase === 'idle' || view.value?.phase === 'preparing'),
);
const mayActivate = computed(
  () => props.allowed && !another.value && view.value?.phase === 'prepared' && !!view.value.attempt,
);

function stopDeferredObservation() {
  observation++;
  clearTimeout(observationTimer);
  observationTimer = undefined;
}

function observeDeferred() {
  if (view.value?.phase !== 'deferred') return;
  const generation = observation;
  const { epoch, effectId } = props;
  const deadline = performance.now() + 5000;
  let remaining = 8;
  const current = () =>
    alive &&
    !busy.value &&
    generation === observation &&
    props.epoch === epoch &&
    props.effectId === effectId &&
    view.value?.phase === 'deferred';

  function schedule() {
    if (!current() || remaining === 0 || performance.now() >= deadline) return;
    observationTimer = setTimeout(() => void read(), 400);
  }

  async function read() {
    observationTimer = undefined;
    if (!current() || performance.now() >= deadline) return;
    remaining--;
    const request = core.effectPeqStatus(epoch, effectId);
    observationRead = request;
    try {
      const next = await request;
      if (current() && next.id === effectId) {
        view.value = next;
        schedule();
      }
    } catch {
      // The displayed receipt remains available for explicit reconciliation.
    } finally {
      if (observationRead === request) observationRead = undefined;
    }
  }
  schedule();
}

async function run(action: () => Promise<EffectPeqView>, mayChangeRegistration = false) {
  if (busy.value || !alive) return;
  const focused = group.value?.contains(document.activeElement);
  stopDeferredObservation();
  confirmation.value = undefined;
  busy.value = true;
  error.value = '';
  try {
    // Status and explicit commands share the native host lock. If a status
    // read already started, let it finish but discard its view.
    if (observationRead) await observationRead.catch(() => undefined);
    if (!alive) return;
    const result = await action();
    if (alive && result.id === props.effectId) view.value = result;
  } catch (issue) {
    if (alive) {
      error.value = failure(issue).message;
      // A failed invoke does not prove the native attempt failed before a write.
      view.value = undefined;
    }
  } finally {
    if (alive) {
      busy.value = false;
      if (mayChangeRegistration) emit('changed');
      observeDeferred();
      await nextTick();
      if (alive && focused && document.activeElement === document.body)
        group.value?.focus({ preventScroll: true });
    }
  }
}

function prepare() {
  if (mayPrepare.value) void run(() => core.effectPeqPrepare(props.epoch, props.effectId));
}
function activate() {
  const attempt = view.value?.attempt;
  if (mayActivate.value && attempt)
    void run(() => core.effectPeqActivate(props.epoch, props.effectId, attempt), true);
}
async function stop() {
  const state = view.value;
  if (busy.value || !state?.canStop || !state.release) return;
  if (!state.stopNeedsRetry) {
    await run(() => core.effectPeqStop(props.epoch, props.effectId, state.release!, false), true);
    return;
  }
  confirmation.value = state.release;
  await nextTick();
  if (alive)
    group.value?.querySelector<HTMLButtonElement>('[data-peq-action="cancel-stop"]')?.focus();
}
function confirmStop() {
  const release = confirmation.value;
  if (release && release === view.value?.release && view.value.stopNeedsRetry)
    void run(() => core.effectPeqStop(props.epoch, props.effectId, release, true), true);
}
async function cancelStop() {
  confirmation.value = undefined;
  await nextTick();
  if (alive) group.value?.querySelector<HTMLButtonElement>('[data-peq-action="stop"]')?.focus();
}

onMounted(() => void run(() => core.effectPeqStatus(props.epoch, props.effectId)));
onBeforeUnmount(() => {
  alive = false;
  stopDeferredObservation();
});
</script>

<template>
  <div ref="group" tabindex="-1" class="effect-parametric" :data-peq-phase="view?.phase">
    <p role="status">
      {{
        busy
          ? '正在处理独立参数均衡…'
          : another
            ? '请先处理另一个音效的独立参数均衡'
            : view
              ? labels[view.phase]
              : '独立参数均衡状态待查询'
      }}
    </p>
    <p v-if="error || view?.issue" role="alert">{{ error || view?.issue }}</p>
    <div class="effect-parametric-actions">
      <button
        v-if="mayPrepare"
        type="button"
        data-peq-action="prepare"
        :aria-disabled="busy"
        @click="prepare"
      >
        {{ view?.phase === 'preparing' ? '继续检查' : '检查独立参数均衡' }}
      </button>
      <button
        v-if="mayActivate"
        type="button"
        data-peq-action="activate"
        :aria-disabled="busy"
        @click="activate"
      >
        启用独立参数均衡
      </button>
      <button
        v-if="view?.canStop && !confirmation"
        type="button"
        data-peq-action="stop"
        :aria-disabled="busy"
        @click="stop"
      >
        {{ view.stopNeedsRetry ? '处理未确认的停止' : '停止并清理登记' }}
      </button>
      <button
        type="button"
        data-peq-action="status"
        :aria-disabled="busy"
        @click="run(() => core.effectPeqReconcile(epoch, effectId))"
      >
        核对状态
      </button>
    </div>
    <template v-if="confirmation">
      <p role="status">上次停止结果尚未确认。再次停止会发送新的停止请求。</p>
      <div class="effect-parametric-actions">
        <button type="button" data-peq-action="confirm-stop" @click="confirmStop">再次停止</button>
        <button type="button" data-peq-action="cancel-stop" @click="cancelStop">取消</button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.effect-parametric {
  border-top: 1px solid var(--line);
  margin-top: 8px;
  padding-top: 8px;
}
.effect-parametric:focus-visible {
  outline: 1px solid var(--accent);
  outline-offset: 3px;
}
.effect-parametric p {
  margin: 0 0 6px;
}
.effect-parametric-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 6px;
}
.effect-parametric-actions button[aria-disabled='true'] {
  opacity: 0.65;
}
</style>
