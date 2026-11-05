import { computed, getCurrentScope, onScopeDispose, ref, watch } from 'vue';

export interface CoverImageSource {
  owner: string;
  source: string | null | undefined;
}

// Official FastImageX (M/182:1320-1344) chooses its default art after one
// source error. The community uses the existing music Icon as that local asset.
// There is no retry, prefetch, alternate URL or extra external fetch here.
export function useCoverImage(current: () => CoverImageSource) {
  const source = ref<string | null>(null);
  const generation = ref(0);
  const state = ref<'empty' | 'loading' | 'ready' | 'error'>('empty');
  let owner: string | undefined;
  let disposed = false;
  const stop = watch(
    current,
    (next) => {
      const url = next.source || null;
      if (owner === next.owner && source.value === url) return;
      owner = next.owner;
      source.value = url;
      generation.value++;
      state.value = url ? 'loading' : 'empty';
    },
    { immediate: true, flush: 'sync' },
  );
  function receive(event: Event, outcome: 'ready' | 'error') {
    const image = event.currentTarget as HTMLImageElement | null;
    if (
      disposed ||
      !source.value ||
      !image ||
      image.getAttribute('data-cover-generation') !== String(generation.value) ||
      image.getAttribute('src') !== source.value
    )
      return;
    state.value = outcome;
  }
  function dispose() {
    disposed = true;
    generation.value++;
    source.value = null;
    state.value = 'empty';
    stop();
  }
  if (getCurrentScope()) onScopeDispose(dispose);
  return {
    source,
    generation,
    state,
    showImage: computed(() => state.value === 'loading' || state.value === 'ready'),
    loaded: computed(() => state.value === 'ready'),
    load: (event: Event) => receive(event, 'ready'),
    error: (event: Event) => receive(event, 'error'),
    dispose,
  };
}
