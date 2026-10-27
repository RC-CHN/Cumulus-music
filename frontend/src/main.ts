import { createApp } from 'vue';
import App from './App.vue';
import { createTauriClient } from './adapters/tauri';
import { coreClientKey } from './services/core';
import './style.css';
import './styles/comments.css';
import './styles/podcast.css';
import './styles/together.css';

createApp(App).provide(coreClientKey, createTauriClient()).mount('#app');
