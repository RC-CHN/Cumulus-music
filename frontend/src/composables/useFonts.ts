import { computed, ref, shallowRef } from 'vue';
import type { FontCatalog, FontClient } from '../services/fonts.ts';
// A dialog owns one account epoch. Closing it invalidates late native responses.
export function useFonts(client: FontClient, epoch: string) {
  const catalog = shallowRef<FontCatalog>(),
    loading = ref(false),
    error = ref('');
  const query = ref(''),
    page = ref(0);
  let disposed = false;
  const filtered = computed(() => {
    const text = query.value.trim().toLocaleLowerCase();
    return (catalog.value?.families ?? []).filter((family) =>
      [family.name, ...family.aliases].some((name) => name.toLocaleLowerCase().includes(text)),
    );
  });
  const pages = computed(() => Math.max(1, Math.ceil(filtered.value.length / 20)));
  const rows = computed(() => filtered.value.slice(page.value * 20, (page.value + 1) * 20));
  async function read() {
    if (disposed || loading.value) return;
    loading.value = true;
    error.value = '';
    try {
      const value = await client.read(epoch);
      if (disposed) return;
      if (value.epoch !== epoch) throw Error('stale');
      catalog.value = value;
      page.value = 0;
    } catch {
      if (!disposed) error.value = '暂时无法读取本机字体，请重试。仍可在设置中手动填写字体名称。';
    } finally {
      if (!disposed) loading.value = false;
    }
  }
  function installed(name: string) {
    const key = name.toLocaleLowerCase();
    return (
      !name ||
      catalog.value?.families.some((family) =>
        [family.name, ...family.aliases].some((value) => value.toLocaleLowerCase() === key),
      )
    );
  }
  return {
    catalog,
    loading,
    error,
    query,
    page,
    pages,
    rows,
    filtered,
    installed,
    read,
    dispose: () => {
      disposed = true;
    },
  };
}
