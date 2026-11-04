import type { Page } from '../composables/useMusic';

export const choiceTabs = [
  { label: '精选', page: { kind: 'ordinary_essential' } },
  { label: '歌单广场', page: { kind: 'playlist_tag', tag: '推荐', choice: true } },
  { label: '排行榜', page: { kind: 'rankings', query: { kind: 'home' }, choice: true } },
  { label: '歌手', page: { kind: 'artist_gallery', choice: true } },
  { label: 'VIP', page: { kind: 'member', choice: true } },
] as const satisfies readonly { label: string; page: Page }[];
