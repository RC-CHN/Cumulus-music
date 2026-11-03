<script setup lang="ts">
import { onBeforeUnmount } from 'vue';
import { useMotion } from '../composables/useMotion';
import { createPageTransition } from '../services/pageTransition';

defineOptions({ inheritAttrs: false });
const props = defineProps<{ name: string }>();
const emit = defineEmits<{ enter: [element: Element] }>();
const motion = useMotion();
const animation = createPageTransition(() => motion.reduced.value, undefined, {
  name: props.name,
  deadlineMs: 360,
});
onBeforeUnmount(animation.dispose);
function enter(element: Element, done: () => void) {
  emit('enter', element);
  animation.enter(element, done);
}
</script>

<template>
  <Transition
    v-bind="$attrs"
    :css="false"
    mode="out-in"
    @enter="enter"
    @leave="animation.leave"
    @enter-cancelled="animation.cancel"
    @leave-cancelled="animation.cancel"
  >
    <slot />
  </Transition>
</template>
