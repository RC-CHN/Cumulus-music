<script setup lang="ts">
import { onMounted, ref, useId } from 'vue';
import Icon from '../Icon.vue';
import ModalTransition from '../ModalTransition.vue';
import NetworkDiagnosticsDialog from '../settings/NetworkDiagnosticsDialog.vue';

const heading = ref<HTMLElement>();
const opener = ref<HTMLButtonElement>();
const diagnose = ref(false);
const title = useId();

onMounted(() => {
  if (document.activeElement === document.body) heading.value?.focus({ preventScroll: true });
});
</script>

<template>
  <section class="empty-state search-offline" data-search-offline :aria-labelledby="title">
    <Icon name="info" />
    <h1 :id="title" ref="heading" tabindex="-1" role="status">网络似乎有问题</h1>
    <button ref="opener" class="secondary" type="button" @click="diagnose = true">网络诊断</button>
    <ModalTransition :return-focus="() => opener ?? null">
      <NetworkDiagnosticsDialog v-if="diagnose" @close="diagnose = false" />
    </ModalTransition>
  </section>
</template>

<style scoped>
.search-offline {
  gap: 14px;
}

.search-offline > .icon {
  width: 48px;
  height: 48px;
}

.search-offline > h1 {
  margin: 0;
}
</style>
