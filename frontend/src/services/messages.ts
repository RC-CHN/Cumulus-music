import type { AccountData } from './contracts';
import type { CommentResource } from './comments';

export const messageTabs = ['private', 'comments', 'forwards', 'notices'] as const;
export type MessageTab = (typeof messageTabs)[number];
export type MessageQuery =
  | { tab: MessageTab; cursor: number | null }
  | { tab: 'thread'; cursor: number | null; peerId: string };

export type MessageCounts = Record<MessageTab, number | null>;
export type MessageTarget =
  | { kind: 'play_track' | 'play_voice' | 'playlist' | 'album' | 'artist' | 'mv'; id: string }
  | { kind: 'comment'; resource: CommentResource; commentId: string };

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
  target: MessageTarget | null;
  deletionReceipt?: string;
  deletionPending?: boolean;
}
export interface MessagePage {
  rows: MessageRow[];
  nextCursor: number | null;
  hint: string | null;
}
export type MessageDeleteOutcome =
  | 'removed'
  | 'uncertain'
  | 'still_listed'
  | 'not_in_page'
  | 'read_unavailable';
export interface MessagePendingDeletion {
  receipt: string;
  conversation: boolean;
}
export interface MessageClient {
  messageCounts(epoch: string): Promise<AccountData<MessageCounts>>;
  messagePage(epoch: string, query: MessageQuery): Promise<AccountData<MessagePage>>;
  pendingMessageDeletions(epoch: string): Promise<AccountData<MessagePendingDeletion[]>>;
  deleteMessage(
    epoch: string,
    receipt: string,
    action: 'remove' | 'check',
  ): Promise<AccountData<MessageDeleteOutcome>>;
}
