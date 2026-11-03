<script setup lang="ts">
import { onUnmounted } from 'vue';
import { useMotion } from '../../composables/useMotion';
import { createPageTransition } from '../../services/pageTransition';

const props = defineProps<{ name: 'tab' | 'reuse-step' }>();
const emit = defineEmits<{
  beforeLeave: [element: Element];
  afterEnter: [element: Element];
}>();
const motion = useMotion();
const animation = createPageTransition(() => motion.reduced.value, undefined, {
  name: props.name,
  deadlineMs: 320,
});
onUnmounted(animation.dispose);
</script>

<template>
  <Transition
    :css="false"
    mode="out-in"
    @enter="animation.enter"
    @enter-cancelled="animation.cancel"
    @after-enter="emit('afterEnter', $event)"
    @before-leave="emit('beforeLeave', $event)"
    @leave="animation.leave"
    @leave-cancelled="animation.cancel"
  >
    <slot />
  </Transition>
</template>
