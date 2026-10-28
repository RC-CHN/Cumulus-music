import { createApp } from 'vue';
import App from './App.vue';
import { createTauriClient } from './adapters/tauri';
import { coreClientKey } from './services/core';
import './style.css';
import './styles/comments.css';
import './styles/podcast.css';
import './styles/together.css';
import { initializeAppearance } from './composables/useAppearance';
import { initializeMotion } from './composables/useMotion';

initializeMotion();
initializeAppearance();
createApp(App).provide(coreClientKey, createTauriClient()).mount('#app');
