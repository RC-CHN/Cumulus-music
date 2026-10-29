import { createApp } from 'vue';
import './style.css';
import './styles/comments.css';
import './styles/podcast.css';
import './styles/together.css';
import { initializeAppearance } from './composables/useAppearance';
import { initializeMotion } from './composables/useMotion';

initializeMotion();
initializeAppearance();
if (/^#mini=\d+$/.test(location.hash)) {
  const { default: MiniApp } = await import('./mini/MiniApp.vue');
  createApp(MiniApp).mount('#app');
} else {
  const [{ default: App }, { createTauriClient }, { coreClientKey }] = await Promise.all([
    import('./App.vue'),
    import('./adapters/tauri'),
    import('./services/core'),
  ]);
  createApp(App).provide(coreClientKey, createTauriClient()).mount('#app');
}
