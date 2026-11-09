import { ref, shallowRef } from 'vue';
import { createInterfaceFontClient } from '../adapters/fonts.ts';
import {
  interfaceFontCss,
  type InterfaceFont,
  type InterfaceFontClient,
} from '../services/fonts.ts';

export function createInterfaceFontPreference(
  client: Pick<InterfaceFontClient, 'read' | 'write'>,
  apply: (value: InterfaceFont) => void,
) {
  const current = shallowRef<InterfaceFont>({ family: null, sizePx: 12 });
  const busy = ref(false);
  const readError = ref(false);
  const writeError = ref(false);

  function accept(value: InterfaceFont) {
    current.value = value;
    apply(value);
  }

  async function load() {
    if (busy.value) return;
    busy.value = true;
    readError.value = false;
    try {
      accept(await client.read());
    } catch {
      readError.value = true;
    } finally {
      busy.value = false;
    }
  }

  async function select(family: string | null) {
    if (busy.value || (!readError.value && !writeError.value && family === current.value.family))
      return false;
    busy.value = true;
    writeError.value = false;
    try {
      accept(await client.write(family));
      readError.value = false;
      return true;
    } catch {
      writeError.value = true;
      return false;
    } finally {
      busy.value = false;
    }
  }

  return { current, busy, readError, writeError, load, select };
}

export function createInterfaceFontCatalog(client: Pick<InterfaceFontClient, 'catalog'>) {
  const choices = shallowRef<string[]>([]);
  const loading = ref(false);
  const failed = ref(false);
  let sequence = 0;

  async function open(epoch: string) {
    const visit = ++sequence;
    choices.value = [];
    loading.value = true;
    failed.value = false;
    try {
      const reply = await client.catalog(epoch);
      if (visit !== sequence) return;
      if (reply.epoch !== epoch) throw Error('font catalog owner changed');
      choices.value = reply.choices;
    } catch {
      if (visit === sequence) failed.value = true;
    } finally {
      if (visit === sequence) loading.value = false;
    }
  }

  return {
    choices,
    loading,
    failed,
    open,
    dispose: () => {
      sequence++;
    },
  };
}

const preference = createInterfaceFontPreference(createInterfaceFontClient(), (value) => {
  const style = document.documentElement.style;
  const css = interfaceFontCss(value);
  if (css.family) style.setProperty('--interface-font-family', css.family);
  else style.removeProperty('--interface-font-family');
  style.setProperty('--interface-font-size', css.size);
});

export const useInterfaceFont = () => preference;
