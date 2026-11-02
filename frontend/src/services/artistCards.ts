import type { ArtistCard } from './catalog.ts';
import type { MenuItem } from './menu.ts';

export interface ArtistCardRequest {
  artist: ArtistCard;
  action: 'collect' | 'copy';
  anchor: HTMLElement | null;
}

/** M622 S.content: myFollow omits favorite; playback has its own avatar control. */
export function artistCardMenu(followed = false): MenuItem[] {
  return [
    { key: 'open', label: '查看歌手', icon: 'user' },
    { key: 'separator', label: '', separator: true },
    ...(!followed ? [{ key: 'collect', label: '收藏', icon: 'star' }] : []),
    { key: 'copy', label: '复制链接', icon: 'copy' },
  ];
}

export function artistLink(id: string): string {
  return /^\d+$/.test(id) ? `https://music.163.com/artist?id=${id}` : '';
}
