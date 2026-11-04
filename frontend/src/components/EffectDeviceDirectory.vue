<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts';
import { useCore } from '../services/core';
import {
  headphoneTuningLabel,
  type EffectDeviceChoice,
  type EffectDeviceMaterialCheck,
  type RememberedHeadphone,
} from '../services/effectDirectory';
import type { BoundParametricStatus, DeviceParametricView } from '../services/player';

const props = defineProps<{
  account: AccountView;
  applied?: DeviceParametricView;
  catalogueParametric?: BoundParametricStatus;
  managementOnly?: boolean;
}>();
const emit = defineEmits<{ browse: [] }>();
const core = useCore();
const brands = ref<EffectDeviceChoice[]>([]);
const models = ref<EffectDeviceChoice[]>([]);
const brand = ref<number>();
const model = ref<number>();
const brandLoading = ref(false);
const modelLoading = ref(false);
const brandError = ref('');
const modelError = ref('');
const material = ref<EffectDeviceMaterialCheck>();
const materialError = ref('');
const materialLoading = ref(false);
const operation = ref<'apply' | 'clear' | 'restore' | 'compare'>();
const operationError = ref('');
const remembered = ref<RememberedHeadphone>();
const rememberedError = ref('');
const rememberedLoading = ref(false);
const applyButton = ref<HTMLButtonElement>();
const clearButton = ref<HTMLButtonElement>();
const browseButton = ref<HTMLButtonElement>();
const restoreButton = ref<HTMLButtonElement>();
const checkButton = ref<HTMLButtonElement>();
const brandSelect = ref<HTMLSelectElement>();
let actionFocus:
  | { action: 'apply' | 'clear'; source: HTMLButtonElement; dialog: HTMLDialogElement | null }
  | undefined;
const busyWithAnotherEffect = computed(() =>
  [props.applied?.status, props.catalogueParametric].some(
    (status) => status && status !== 'processor_failed',
  ),
);
const selectedTuning = computed(
  () =>
    !!props.applied && props.applied.brand === brand.value && props.applied.model === model.value,
);
const materialLabels: Record<EffectDeviceMaterialCheck, string> = {
  no_material: '此型号暂无可检查的调音材料。',
  pending: '调音材料尚未检查完，可继续检查。',
  standalone_peq: '已验证此型号的参数均衡，可以在当前播放会话中启用。',
  unsupported_graph: '已验证调音材料，当前无法应用其中的效果组合。',
};
let brandRequest = 0;
let modelRequest = 0;
let materialRequest = 0;
let rememberedRequest = 0;
let directoryGeneration = 0;
const cachedModels = new Map<number, EffectDeviceChoice[]>();
const modelReads = new Map<number, Promise<EffectDeviceChoice[]>>();
const modelCacheLimit = 8;

function owns(epoch: string) {
  return props.account.status === 'authenticated' && props.account.epoch === epoch;
}
async function readRemembered() {
  const epoch = props.account.epoch;
  const request = ++rememberedRequest;
  const current = () => owns(epoch) && props.managementOnly && rememberedRequest === request;
  rememberedLoading.value = true;
  rememberedError.value = '';
  try {
    const result = await core.effectDeviceRemembered(epoch);
    if (current()) remembered.value = result ?? undefined;
  } catch (issue) {
    if (current()) {
      remembered.value = undefined;
      rememberedError.value = failure(issue).message;
    }
  } finally {
    if (current()) rememberedLoading.value = false;
  }
}
async function loadBrands() {
  const epoch = props.account.epoch;
  const request = ++brandRequest;
  const current = () => owns(epoch) && request === brandRequest;
  brandError.value = '';
  brandLoading.value = true;
  try {
    const rows = await core.effectDeviceBrands(epoch);
    if (current()) brands.value = rows;
  } catch (issue) {
    if (current()) brandError.value = failure(issue).message;
  } finally {
    if (current()) brandLoading.value = false;
  }
}
async function loadModels(id?: number) {
  const epoch = props.account.epoch;
  const request = ++modelRequest;
  const current = () => owns(epoch) && request === modelRequest;
  brand.value = id;
  model.value = undefined;
  resetMaterial();
  models.value = [];
  modelError.value = '';
  modelLoading.value = id !== undefined;
  if (id === undefined) return;
  const cached = cachedModels.get(id);
  if (cached) {
    cachedModels.delete(id);
    cachedModels.set(id, cached);
    models.value = cached;
    modelLoading.value = false;
    return;
  }
  const generation = directoryGeneration;
  let read = modelReads.get(id);
  if (!read) {
    read = core.effectDeviceModels(epoch, id).then((rows) => {
      if (generation === directoryGeneration && owns(epoch)) {
        if (cachedModels.size >= modelCacheLimit)
          cachedModels.delete(cachedModels.keys().next().value!);
        cachedModels.set(id, rows);
      }
      return rows;
    });
    modelReads.set(id, read);
  }
  try {
    const rows = await read;
    if (current()) models.value = rows;
  } catch (issue) {
    if (current()) modelError.value = failure(issue).message;
  } finally {
    if (modelReads.get(id) === read) modelReads.delete(id);
    if (current()) modelLoading.value = false;
  }
}
function chooseBrand(event: Event) {
  const value = (event.currentTarget as HTMLSelectElement).value;
  void loadModels(brands.value.find((item) => String(item.id) === value)?.id);
}
function chooseModel(event: Event) {
  const value = (event.currentTarget as HTMLSelectElement).value;
  model.value = models.value.find((item) => String(item.id) === value)?.id;
  resetMaterial();
}
function resetMaterial() {
  materialRequest++;
  material.value = undefined;
  materialError.value = '';
  materialLoading.value = false;
  operationError.value = '';
}
async function checkMaterial() {
  if (materialLoading.value || brand.value === undefined || model.value === undefined) return;
  const epoch = props.account.epoch;
  const request = ++materialRequest;
  const current = () => owns(epoch) && request === materialRequest;
  materialError.value = '';
  materialLoading.value = true;
  try {
    const result = await core.effectDeviceMaterial(epoch, brand.value, model.value);
    if (current()) material.value = result;
  } catch (issue) {
    if (current()) {
      material.value = undefined;
      materialError.value = failure(issue).message;
    }
  } finally {
    if (current()) materialLoading.value = false;
  }
}
async function changeTuning(action: 'apply' | 'clear', event?: MouseEvent) {
  const applied = props.applied;
  if (
    action === 'clear' &&
    (!applied ||
      applied.activationId !== (event?.currentTarget as HTMLButtonElement)?.dataset.activationId)
  )
    return;
  const target = action === 'clear' ? applied : { brand: brand.value, model: model.value };
  if (operation.value || target?.brand === undefined || target.model === undefined) return;
  if (action === 'apply' && (material.value !== 'standalone_peq' || busyWithAnotherEffect.value))
    return;
  const epoch = props.account.epoch;
  const generation = directoryGeneration;
  const current = () => owns(epoch) && directoryGeneration === generation;
  const source = action === 'apply' ? applyButton.value : clearButton.value;
  if (source && document.activeElement === source)
    actionFocus = { action, source, dialog: source.closest('dialog') };
  operation.value = action;
  operationError.value = '';
  try {
    if (action === 'apply') await core.effectDeviceApply(epoch, target.brand, target.model);
    else if (applied)
      await core.effectDeviceClear(epoch, target.brand, target.model, applied.activationId);
  } catch (issue) {
    if (current()) {
      operationError.value = failure(issue).message;
      actionFocus = undefined;
    }
  } finally {
    if (current()) operation.value = undefined;
  }
}
async function restoreRemembered(event: MouseEvent) {
  const source = event.currentTarget as HTMLButtonElement;
  const choice = remembered.value;
  if (
    !choice ||
    source.dataset.rememberedRevision !== choice.revision ||
    operation.value ||
    busyWithAnotherEffect.value ||
    !props.managementOnly
  )
    return;
  const epoch = props.account.epoch;
  const generation = directoryGeneration;
  const current = () => owns(epoch) && directoryGeneration === generation;
  if (document.activeElement === source)
    actionFocus = { action: 'apply', source, dialog: source.closest('dialog') };
  operation.value = 'restore';
  operationError.value = '';
  try {
    await core.effectDeviceRestore(epoch, choice.revision);
    if (current()) await readRemembered();
  } catch (issue) {
    if (current()) {
      operationError.value = failure(issue).message;
      actionFocus = undefined;
      if (failure(issue).code === 'stale') await readRemembered();
    }
  } finally {
    if (current()) operation.value = undefined;
  }
}
async function compareTuning(event: MouseEvent) {
  const applied = props.applied;
  const comparison = applied?.comparison;
  if (
    operation.value ||
    !applied ||
    !comparison ||
    (event.currentTarget as HTMLButtonElement).dataset.comparisonReceipt !== comparison.receipt
  )
    return;
  const epoch = props.account.epoch;
  const generation = directoryGeneration;
  const current = () => owns(epoch) && directoryGeneration === generation;
  operation.value = 'compare';
  operationError.value = '';
  try {
    await core.effectDeviceCompare(epoch, {
      ...comparison,
      brand: applied.brand,
      model: applied.model,
      activationId: applied.activationId,
      paused: applied.status !== 'paused_locally',
    });
  } catch (issue) {
    if (current()) operationError.value = failure(issue).message;
  } finally {
    if (current()) operation.value = undefined;
  }
}
function resetReads() {
  brandRequest++;
  modelRequest++;
  directoryGeneration++;
  rememberedRequest++;
  cachedModels.clear();
  modelReads.clear();
  operation.value = undefined;
  remembered.value = undefined;
  rememberedLoading.value = false;
  rememberedError.value = '';
  actionFocus = undefined;
  resetMaterial();
}
watch(
  () => [props.applied?.status, operation.value] as const,
  ([status, running]) => {
    if (!actionFocus || running) return;
    const { action, source, dialog } = actionFocus;
    if (action === 'apply' ? !status || status === 'processor_failed' : !!status) return;
    actionFocus = undefined;
    if (
      document.activeElement !== source &&
      document.activeElement !== document.body &&
      document.activeElement !== dialog
    )
      return;
    const target =
      action === 'apply'
        ? clearButton.value
        : (applyButton.value ??
          checkButton.value ??
          brandSelect.value ??
          browseButton.value ??
          restoreButton.value);
    target?.focus({ preventScroll: true });
  },
  { flush: 'post' },
);
watch(
  () => [props.account.epoch, props.account.status, props.managementOnly] as const,
  ([epoch]) => {
    resetReads();
    brands.value = [];
    models.value = [];
    brand.value = model.value = undefined;
    brandError.value = modelError.value = '';
    brandLoading.value = modelLoading.value = false;
    if (owns(epoch)) {
      if (props.managementOnly) void readRemembered();
      else void loadBrands();
    }
  },
  { immediate: true, flush: 'sync' },
);
onUnmounted(resetReads);
</script>

<template>
  <section class="effect-device-directory" aria-label="耳机音效目录">
    <p v-if="!managementOnly">选定型号后可检查和应用调音。启用后仍使用当前播放设备。</p>
    <div v-if="applied" class="effect-device-active" :data-device-tuning="applied.status">
      <p role="status">
        {{ headphoneTuningLabel(applied.status) }} ·
        {{
          applied.modelName ??
          (brand === applied.brand
            ? (models.find((item) => item.id === applied?.model)?.name ?? `型号 ${applied.model}`)
            : `品牌 ${applied.brand} · 型号 ${applied.model}`)
        }}
      </p>
      <button
        v-if="applied.comparison"
        type="button"
        :data-comparison-receipt="applied.comparison.receipt"
        :aria-disabled="!!operation"
        @click="compareTuning"
      >
        {{ applied.status === 'paused_locally' ? '继续当前调音' : '暂时停用调音' }}
      </button>
      <button
        ref="clearButton"
        type="button"
        :key="applied.activationId"
        :data-activation-id="applied.activationId"
        :disabled="!!operation"
        @click="changeTuning('clear', $event)"
      >
        {{ applied.status === 'processor_failed' ? '关闭调音提示' : '关闭耳机调音' }}
      </button>
    </div>
    <p v-if="operationError" role="alert">{{ operationError }}</p>
    <p v-if="applied?.selectionSaveFailed" role="status">本次无法记住此型号。</p>
    <template v-if="managementOnly">
      <p v-if="!applied" role="status">当前没有耳机调音。</p>
      <p v-if="rememberedLoading" role="status">正在读取上次使用的耳机…</p>
      <div v-else-if="rememberedError">
        <p role="alert">{{ rememberedError }}</p>
        <button type="button" @click="readRemembered">重新读取上次记录</button>
      </div>
      <div v-else-if="remembered" class="effect-device-remembered">
        <p>上次使用 · 品牌 {{ remembered.brand }} · 型号 {{ remembered.model }}</p>
        <p v-if="!applied && busyWithAnotherEffect">请先关闭正在使用的参数均衡。</p>
        <button
          ref="restoreButton"
          type="button"
          :key="remembered.revision"
          :data-remembered-revision="remembered.revision"
          :disabled="!!operation || busyWithAnotherEffect"
          @click="restoreRemembered"
        >
          {{ operation === 'restore' ? '正在恢复…' : '恢复耳机调音' }}
        </button>
      </div>
      <div class="effect-device-actions">
        <button ref="browseButton" type="button" @click="emit('browse')">浏览耳机音效</button>
        <slot name="management-actions" />
      </div>
    </template>
    <p v-else-if="brandLoading" role="status">正在读取耳机品牌…</p>
    <div v-else-if="brandError">
      <p role="alert">{{ brandError }}</p>
      <button type="button" @click="loadBrands">重试品牌</button>
    </div>
    <p v-else-if="!brands.length">暂无耳机品牌。</p>
    <template v-else>
      <label>
        耳机品牌
        <select
          ref="brandSelect"
          :value="brand ?? ''"
          :disabled="!!operation"
          @change="chooseBrand"
        >
          <option value="">请选择品牌</option>
          <option v-for="item in brands" :key="item.id" :value="item.id">{{ item.name }}</option>
        </select>
      </label>
      <p v-if="modelLoading" role="status">正在读取耳机型号…</p>
      <div v-else-if="modelError">
        <p role="alert">{{ modelError }}</p>
        <button type="button" @click="loadModels(brand)">重试型号</button>
      </div>
      <p v-else-if="brand !== undefined && !models.length">此品牌暂无耳机型号。</p>
      <label v-else-if="models.length">
        耳机型号
        <select :value="model ?? ''" :disabled="!!operation" @change="chooseModel">
          <option value="">请选择型号</option>
          <option v-for="item in models" :key="item.id" :value="item.id">{{ item.name }}</option>
        </select>
      </label>
      <div
        v-if="model !== undefined"
        class="effect-device-material"
        :data-material-check="material"
      >
        <div class="effect-device-actions">
          <button
            ref="checkButton"
            type="button"
            :disabled="materialLoading || !!operation"
            @click="checkMaterial"
          >
            {{
              material === 'pending'
                ? '继续检查材料'
                : materialError
                  ? '重试检查材料'
                  : '检查调音材料'
            }}
          </button>
          <button
            v-if="material === 'standalone_peq' && !materialLoading && !busyWithAnotherEffect"
            ref="applyButton"
            type="button"
            class="effect-device-apply"
            :disabled="!!operation"
            @click="changeTuning('apply')"
          >
            {{ operation === 'apply' ? '正在启用…' : '启用耳机调音' }}
          </button>
        </div>
        <p v-if="materialLoading" role="status">正在检查所选型号的调音材料…</p>
        <p v-else-if="materialError" role="alert">{{ materialError }}</p>
        <p v-else-if="material === 'standalone_peq' && busyWithAnotherEffect && !selectedTuning">
          请先关闭当前参数均衡，再启用此型号的调音。
        </p>
        <p
          v-else-if="material && (!selectedTuning || applied?.status === 'processor_failed')"
          role="status"
        >
          {{ materialLabels[material] }}
        </p>
      </div>
    </template>
    <div v-if="!managementOnly && $slots['management-actions']" class="effect-device-secondary">
      <slot name="management-actions" />
    </div>
  </section>
</template>

<style scoped>
.effect-device-directory p {
  color: var(--text-secondary);
}
.effect-device-directory label {
  display: grid;
  grid-template-columns: 70px minmax(0, 260px);
  align-items: center;
  gap: 10px;
  margin-block: 12px;
}
.effect-device-directory select {
  min-width: 0;
  padding: 7px 10px;
  color: var(--text);
  background: var(--surface-alt);
  border: 1px solid var(--line);
  border-radius: 6px;
}
.effect-device-active,
.effect-device-actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.effect-device-active {
  margin-block: 8px 12px;
  padding: 8px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface-alt);
}
.effect-device-active p {
  flex: 1 1 12rem;
  min-width: 0;
  margin: 0;
  overflow-wrap: anywhere;
  color: var(--text);
  font-weight: 500;
}
.effect-device-material p {
  margin: 8px 0 0;
}
.effect-device-secondary {
  margin-top: 12px;
}
.effect-device-directory button {
  min-height: 34px;
  padding: 4px 12px;
  border: 1px solid var(--line);
  border-radius: 999px;
}
.effect-device-directory button:focus-visible,
.effect-device-directory button:hover:not(:disabled) {
  border-color: var(--accent);
}
.effect-device-directory .effect-device-apply {
  color: var(--accent);
  border-color: var(--accent);
  font-weight: 600;
}
</style>
