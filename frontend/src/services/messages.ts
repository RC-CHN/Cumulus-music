import type { AccountData } from './contracts';

export const messageTabs = ['private', 'comments', 'forwards', 'notices'] as const;
export type MessageTab = (typeof messageTabs)[number];
export type MessageQuery =
  | { tab: MessageTab; cursor: number | null }
  | { tab: 'thread'; cursor: number | null; peerId: string };

export type MessageCounts = Record<MessageTab, number | null>;
export interface MessageRow {
  id: string;
  peerId: string | null;
  author: string;
  body: string;
  caption: string | null;
  quote: string | null;
  time: number | null;
  unread: number | null;
  own: boolean;
}
export interface MessagePage {
  rows: MessageRow[];
  nextCursor: number | null;
  hint: string | null;
}
export interface MessageClient {
  messageCounts(epoch: string): Promise<AccountData<MessageCounts>>;
  messagePage(epoch: string, query: MessageQuery): Promise<AccountData<MessagePage>>;
}
