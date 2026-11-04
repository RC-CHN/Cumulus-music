import { computed, ref, shallowRef, watch } from 'vue';
import { useMotion } from './useMotion';
import {
  appearanceKey,
  customAccent,
  readAppearance,
  resolvedAppearance,
  storeAppearance,
  type AppearanceSelection,
  type BuiltinAppearance,
} from '../services/appearance';

const selection = shallowRef<AppearanceSelection>({ theme: 'system' });
const preference = computed(() => selection.value.theme);
const customColor = computed(() =>
  selection.value.theme === 'custom' ? selection.value.color : undefined,
);
const preview = shallowRef<{ owner: symbol; color: string }>();
const systemDark = ref(false);
const saved = ref(true);
let installed = false;
let transition: Animation | undefined;
const motion = useMotion();
watch(
  motion.reduced,
  (reduced) => {
    if (reduced) transition?.cancel();
  },
  { flush: 'sync' },
);
const effective = computed(() =>
  resolvedAppearance(preview.value ? 'custom' : preference.value, systemDark.value),
);
function apply() {
  const root = document.documentElement;
  root.toggleAttribute('data-theme-preview', !!preview.value);
  const color = preview.value?.color ?? customColor.value;
  if (color) root.style.setProperty('--custom-theme-color', color);
  else root.style.removeProperty('--custom-theme-color');
  root.dataset.theme = effective.value;
}
export function initializeAppearance() {
  if (installed) return;
  installed = true;
  try {
    selection.value = readAppearance(localStorage.getItem(appearanceKey));
  } catch {
    saved.value = false;
  }
  const query = matchMedia('(prefers-color-scheme: dark)');
  systemDark.value = query.matches;
  query.addEventListener('change', (event) => {
    systemDark.value = event.matches;
    apply();
  });
  window.addEventListener('storage', (event) => {
    if (event.key === appearanceKey || event.key === null) {
      selection.value = readAppearance(event.newValue);
      saved.value = true;
      apply();
    }
  });
  apply();
}
function persist(value: AppearanceSelection) {
  const before = effective.value;
  preview.value = undefined;
  selection.value = value;
  try {
    localStorage.setItem(appearanceKey, storeAppearance(value));
    saved.value = true;
  } catch {
    saved.value = false;
  }
  apply();
  transition?.cancel();
  // Fade the existing compositor layer; keep the live UI and playback mounted.
  if (before !== effective.value && !motion.reduced.value)
    transition = document
      .querySelector('.app-shell')
      ?.animate([{ opacity: 0.65 }, { opacity: 1 }], { duration: 180, easing: 'ease-out' });
}
function select(value: BuiltinAppearance) {
  persist({ theme: value });
}
function beginCustomPreview() {
  const owner = Symbol();
  let active = true;
  return {
    change(color: string) {
      const validated = customAccent(color);
      if (!active || !validated) return;
      preview.value = { owner, color: validated };
      apply();
    },
    commit(color: string) {
      const validated = customAccent(color);
      if (!active || !validated) return;
      active = false;
      persist({ theme: 'custom', color: validated });
    },
    cancel() {
      active = false;
      if (preview.value?.owner !== owner) return;
      preview.value = undefined;
      apply();
    },
  };
}
export function useAppearance() {
  return { preference, effective, saved, select, customColor, beginCustomPreview };
}
