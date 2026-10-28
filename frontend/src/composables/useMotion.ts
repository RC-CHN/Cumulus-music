import { computed, ref } from 'vue';
import {
  motionKey,
  motionPreference,
  reducedMotion,
  type MotionPreference,
} from '../services/motion.ts';

interface MotionEnvironment {
  read(): string | null;
  write(value: MotionPreference): void;
  apply(reduced: boolean): void;
}

// Presentation-only, device-local preference. It never follows an account or
// changes playback clocks, network requests, or business operation deadlines.
export function createMotionPreferences(environment: MotionEnvironment) {
  const preference = ref<MotionPreference>('system');
  const systemReduced = ref(false);
  const saved = ref(true);
  const reduced = computed(() => reducedMotion(preference.value, systemReduced.value));
  function apply() {
    environment.apply(reduced.value);
  }
  function initialize(system: boolean) {
    systemReduced.value = system;
    try {
      preference.value = motionPreference(environment.read());
    } catch {
      saved.value = false;
    }
    apply();
  }
  function select(disabled: boolean) {
    preference.value = disabled ? 'disabled' : 'system';
    try {
      environment.write(preference.value);
      saved.value = true;
    } catch {
      saved.value = false;
    }
    apply();
  }
  function systemChanged(value: boolean) {
    systemReduced.value = value;
    apply();
  }
  function storedChanged(value: string | null) {
    preference.value = motionPreference(value);
    saved.value = true;
    apply();
  }
  return {
    preference,
    systemReduced,
    reduced,
    saved,
    initialize,
    select,
    systemChanged,
    storedChanged,
  };
}

const motion = createMotionPreferences({
  read: () => localStorage.getItem(motionKey),
  write: (value) => localStorage.setItem(motionKey, value),
  apply: (reduced) => {
    document.documentElement.dataset.motion = reduced ? 'reduced' : 'full';
  },
});
let installed = false;
export function initializeMotion() {
  if (installed) return;
  installed = true;
  const query = matchMedia('(prefers-reduced-motion: reduce)');
  motion.initialize(query.matches);
  query.addEventListener('change', (event) => motion.systemChanged(event.matches));
  window.addEventListener('storage', (event) => {
    if (event.key === motionKey || event.key === null) {
      // Ignore unrelated sessionStorage events; accessing localStorage can fail
      // in a restricted WebView, which must not prevent rendering.
      try {
        if (event.storageArea !== localStorage) return;
      } catch {
        return;
      }
      motion.storedChanged(event.newValue);
    }
  });
}
export function useMotion() {
  return motion;
}
