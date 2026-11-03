<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
import { failure, type AccountView } from '../services/contracts';
import { useCore } from '../services/core';
import type { EffectDeviceChoice } from '../services/effectDirectory';

const props = defineProps<{ account: AccountView }>();
const core = useCore();
const brands = ref<EffectDeviceChoice[]>([]);
const models = ref<EffectDeviceChoice[]>([]);
const brand = ref<number>();
const model = ref<number>();
const brandLoading = ref(false);
const modelLoading = ref(false);
const brandError = ref('');
const modelError = ref('');
let brandRequest = 0;
let modelRequest = 0;

function owns(epoch: string) {
  return props.account.status === 'authenticated' && props.account.epoch === epoch;
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
  models.value = [];
  modelError.value = '';
  modelLoading.value = id !== undefined;
  if (id === undefined) return;
  try {
    const rows = await core.effectDeviceModels(epoch, id);
    if (current()) models.value = rows;
  } catch (issue) {
    if (current()) modelError.value = failure(issue).message;
  } finally {
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
}
onMounted(() => void loadBrands());
onUnmounted(() => {
  brandRequest++;
  modelRequest++;
});
</script>

<template>
  <section class="effect-device-directory" aria-label="耳机音效目录">
    <p>可查看品牌和型号。选择型号不会更改播放设备或应用耳机调音。</p>
    <p v-if="brandLoading" role="status">正在读取耳机品牌…</p>
    <div v-else-if="brandError">
      <p role="alert">{{ brandError }}</p>
      <button type="button" @click="loadBrands">重试品牌</button>
    </div>
    <p v-else-if="!brands.length">暂无耳机品牌。</p>
    <template v-else>
      <label>
        耳机品牌
        <select :value="brand ?? ''" @change="chooseBrand">
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
        <select :value="model ?? ''" @change="chooseModel">
          <option value="">请选择型号</option>
          <option v-for="item in models" :key="item.id" :value="item.id">{{ item.name }}</option>
        </select>
      </label>
    </template>
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
</style>
