import type { AccountData } from './contracts';
import type { CommunityCard } from './community';

export type CommunityTopicAction =
  | { kind: 'hot' | 'first' | 'refresh' }
  | { kind: 'next'; continuation: string };
export interface CommunityTopic {
  handle: string;
  title: string;
  description: string | null;
  coverUrl: string | null;
  wideCoverUrl: string | null;
  participants: number | null;
}
export interface CommunityTopicSlice {
  cards: CommunityCard[];
  continuation: string | null;
  replace: boolean;
  unavailable: boolean;
  retired: string[];
}
export interface CommunityTopicClient {
  communityTopic(epoch: string, name: string): Promise<AccountData<CommunityTopic | null>>;
  communityTopicNotice(epoch: string, topic: string): Promise<AccountData<boolean>>;
  communityTopicSlice(
    epoch: string,
    topic: string,
    action: CommunityTopicAction,
  ): Promise<AccountData<CommunityTopicSlice>>;
}

export type TopicPart = { text: string; topic?: string };
export function topicParts(value: string | null): TopicPart[] {
  if (!value) return [];
  const parts: TopicPart[] = [];
  let cursor = 0;
  let links = 0;
  for (const item of value.matchAll(/#([^\[\]/\\#\r\n]+?)#/gu)) {
    const index = item.index;
    const name = item[1];
    if (!name.trim() || [...name].length > 80 || /\p{Cc}/u.test(name)) continue;
    if (links++ >= 24) break;
    if (index > cursor) parts.push({ text: value.slice(cursor, index) });
    parts.push({ text: item[0], topic: name });
    cursor = index + item[0].length;
  }
  if (cursor < value.length) parts.push({ text: value.slice(cursor) });
  return parts;
}
