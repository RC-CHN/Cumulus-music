import { computed, ref } from 'vue';
import {
  appearanceKey,
  appearancePreference,
  resolvedAppearance,
  type AppearancePreference,
} from '../services/appearance';

const preference = ref<AppearancePreference>('system');
const systemDark = ref(false);
const saved = ref(true);
let installed = false;
let transition: Animation | undefined;
const effective = computed(() => resolvedAppearance(preference.value, systemDark.value));
function apply() {
  document.documentElement.dataset.theme = effective.value;
}
export function initializeAppearance() {
  if (installed) return;
  installed = true;
  try {
    preference.value = appearancePreference(localStorage.getItem(appearanceKey));
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
      preference.value = appearancePreference(event.newValue);
      apply();
    }
  });
  apply();
}
function select(value: AppearancePreference) {
  const before = effective.value;
  preference.value = value;
  try {
    localStorage.setItem(appearanceKey, value);
    saved.value = true;
  } catch {
    saved.value = false;
  }
  apply();
  transition?.cancel();
  // WebKitGTK 2.52.6 crashes in its UI process during startViewTransition.
  // Fade the existing compositor layer; keep the live UI and playback mounted.
  if (before !== effective.value && !matchMedia('(prefers-reduced-motion: reduce)').matches)
    transition = document
      .querySelector('.app-shell')
      ?.animate([{ opacity: 0.65 }, { opacity: 1 }], { duration: 180, easing: 'ease-out' });
}
export function useAppearance() {
  return { preference, effective, saved, select };
}
