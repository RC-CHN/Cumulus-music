<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { createInterfaceFontClient } from '../../adapters/fonts';
import { createInterfaceFontCatalog, useInterfaceFont } from '../../composables/useInterfaceFont';
import { interfaceFontPage, interfaceFontPageSize } from '../../services/fonts';

const props = defineProps<{ epoch: string }>();
const preference = useInterfaceFont();
const catalog = createInterfaceFontCatalog(createInterfaceFontClient());
const page = ref(0);
const saved = ref(false);
let feedback: ReturnType<typeof setTimeout> | undefined;
let retired = false;
const selected = computed(() => preference.current.value.family);
const available = computed(() => new Set(catalog.choices.value));
const pages = computed(() => Math.ceil(catalog.choices.value.length / interfaceFontPageSize));
const visible = computed(() =>
  interfaceFontPage(catalog.choices.value, selected.value, page.value),
);

function open() {
  page.value = 0;
  void catalog.open(props.epoch);
}
watch(() => props.epoch, open, { immediate: true });
onBeforeUnmount(() => {
  retired = true;
  clearTimeout(feedback);
  catalog.dispose();
});

async function choose(event: Event) {
  const control = event.currentTarget as HTMLSelectElement;
  const value = control.value;
  const epoch = props.epoch;
  if ((!value || available.value.has(value)) && (await preference.select(value || null))) {
    if (!retired && props.epoch === epoch) {
      saved.value = true;
      clearTimeout(feedback);
      feedback = setTimeout(() => (saved.value = false), 1000);
    }
  }
  control.value = selected.value ?? '';
}
</script>

<template>
  <div class="interface-font-setting">
    <label for="interface-font">字体选择</label>
    <select
      id="interface-font"
      data-interface-font
      :value="selected ?? ''"
      :aria-busy="catalog.loading.value || preference.busy.value"
      @change="choose"
    >
      <option value="">默认</option>
      <option
        v-for="family in visible"
        :key="family"
        :value="family"
        :disabled="!available.has(family)"
      >
        {{ family }}
      </option>
    </select>
    <nav v-if="pages > 1" class="interface-font-pages" aria-label="界面字体分页">
      <button type="button" class="secondary" :disabled="page === 0" @click="page--">上一页</button>
      <span>{{ page + 1 }} / {{ pages }}</span>
      <button type="button" class="secondary" :disabled="page + 1 === pages" @click="page++">
        下一页
      </button>
    </nav>
    <p v-if="catalog.failed.value" class="settings-note" role="alert">
      暂时无法读取本机字体。
      <button type="button" class="secondary" @click="open">重新读取</button>
    </p>
    <p v-if="preference.readError.value" class="settings-note" role="alert">
      暂时无法读取已保存的界面字体。
    </p>
    <p v-if="preference.writeError.value" class="settings-note" role="alert">
      字体未保存，仍使用原来的字体。
    </p>
    <p v-else-if="saved" class="settings-note" role="status">设置已更新</p>
  </div>
</template>
