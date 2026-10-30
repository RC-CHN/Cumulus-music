<script setup lang="ts">
defineProps<{ name: string }>();
const paths: Record<string, string> = {
  plus: 'M12 4v16M4 12h16',
  copy: 'M9 9h12v12H9zM15 6V3H3v12h3',
  trash: 'M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7m4-7v7',
  settings:
    'm9 3-.5 3-2 1-3-.8-2 3 2.5 2v2L1.5 15l2 3 3-.8 2 1L9 21h4l.5-2.8 2-1 3 .8 2-3-2.5-1.8v-2l2.5-2.2-2-3-3 .8-2-1L13 3zM15 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0Z',
  history: 'M3 4v6h6M3 10a9 9 0 1 1 1 7m8-12v7l4 2',
  edit: 'm16 3 5 5M4 20l4-1L21 6l-4-4L4 15z',
  check: 'm5 12 4 4L19 6',
  sun: 'M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.5 1.5m11 11L19 19M5 19l1.5-1.5m11-11L19 5M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0Z',
  moon: 'M20 15a9 9 0 0 1-11-11A9 9 0 1 0 20 15Z',
  monitor: 'M3 4h18v13H3zM8 21h8m-4-4v4',
  palette:
    'M12 3a9 9 0 1 0 0 18h1a2 2 0 0 0 0-4 2 2 0 0 1 0-4h4a4 4 0 0 0 4-4c0-4-5-6-9-6ZM7 10h.01M10 7h.01M15 7h.01',
  star: 'm12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9z',
  download: 'M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5',
  headphones: 'M4 14v-3a8 8 0 0 1 16 0v3M4 12H2v8h5v-8zM20 12h2v8h-5v-8z',
  comment: 'M4 4h16v12H9l-5 4zM8 8h8M8 12h5',
  thumb: 'M8 10l4-7h3v7h6l-2 11H8zM3 10h5v11H3z',
  video: 'M3 5h13v14H3zM16 9l5-3v12l-5-3',
  cloud: 'M7 18h11a4 4 0 0 0 .2-8 6 6 0 0 0-11.5-2A5 5 0 0 0 7 18Z',
  play: 'm8 4 13 8-13 8z',
  pause: 'M8 5v14M16 5v14',
  stop: 'M5 5h14v14H5z',
  previous: 'M5 4v16M20 4 7 12l13 8z',
  next: 'M19 4v16M4 4l13 8-13 8z',
  repeat: 'm17 2 4 4-4 4M3 11V8a2 2 0 0 1 2-2h16M7 22l-4-4 4-4m14-1v3a2 2 0 0 1-2 2H3',
  repeatOne: 'm17 2 4 4-4 4M3 11V8a2 2 0 0 1 2-2h16M7 22l-4-4 4-4m14-1v3a2 2 0 0 1-2 2H3m8-8 2-1v6',
  shuffle: 'M3 5h3l12 14h3m-4-4 4 4-4 3M3 19h3l12-14h3m-4-3 4 3-4 4',
  list: 'M8 5h13M8 12h13M8 19h8M3 5h.01M3 12h.01M3 19h.01',
  volume: 'M11 4 5 9H2v6h3l6 5zM15 8a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14',
  muted: 'M11 4 5 9H2v6h3l6 5zM16 9l6 6m0-6-6 6',
  up: 'm5 15 7-7 7 7',
  down: 'm5 9 7 7 7-7',
  music:
    'M9 18V5l12-2v13M9 8l12-2M9 18a3 3 0 1 1-3-3c1.7 0 3 1.3 3 3Zm12-2a3 3 0 1 1-3-3c1.7 0 3 1.3 3 3Z',
  search: 'm21 21-5.2-5.2M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z',
  heart:
    'M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.7l-1.1-1.1a5.5 5.5 0 0 0-7.8 7.8L12 21l8.8-8.6a5.5 5.5 0 0 0 0-7.8Z',
  grid: 'M3 3h7v7H3zM14 3h7v7h-7zM3 14h7v7H3zM14 14h7v7h-7z',
  calendar: 'M5 4h14a2 2 0 0 1 2 2v14H3V6a2 2 0 0 1 2-2Zm2-2v4m10-4v4M3 10h18M7 14h3m4 0h3m-10 3h3',
  back: 'm15 5-7 7 7 7',
  forward: 'm9 5 7 7-7 7',
  close: 'm6 6 12 12M6 18 18 6',
  refresh: 'M20 7a9 9 0 1 0 1 8M20 2v5h-5',
  user: 'M20 21a8 8 0 0 0-16 0M16 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0Z',
  lock: 'M5 10h14v11H5zM8 10V6a4 4 0 0 1 8 0v4',
};
</script>
<template>
  <svg
    class="icon"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.65"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <g v-if="name === 'more'" fill="currentColor" stroke="none">
      <circle cx="5" cy="12" r="1.6" />
      <circle cx="12" cy="12" r="1.6" />
      <circle cx="19" cy="12" r="1.6" />
    </g>
    <path v-else :d="paths[name] || paths.music" />
  </svg>
</template>
