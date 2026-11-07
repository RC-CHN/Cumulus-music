import type { AccountData } from './contracts';
import type { CommentResource } from './comments';

export const messageTabs = ['private', 'comments', 'forwards', 'notices'] as const;
export type MessageTab = (typeof messageTabs)[number];
export type MessageQuery =
  | { tab: MessageTab; cursor: number | null }
  | { tab: 'thread'; cursor: number | null; peerId: string };

export type MessageCounts = Record<MessageTab, number | null>;
export type MessageCountNavigation = MessageTab | 'conversation';
export interface MessageCountView {
  epoch: string;
  counts: MessageCounts | null;
  hidePrivateRowBadges: boolean;
  drawerVisible: boolean;
  refreshing: boolean;
  failed: boolean;
}
export interface MessageCountStream {
  visibility(epoch: string, sequence: number, visible: boolean): Promise<void>;
  navigate(epoch: string, sequence: number, navigation: MessageCountNavigation): Promise<void>;
  refresh(epoch: string): Promise<void>;
  close(): void;
}
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
  replyReceipt?: string;
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
export type MessageReplyAction =
  | { kind: 'status'; receipt: string }
  | { kind: 'send'; receipt: string; text: string };
export type MessageReplyView = { receipt: string } & (
  | { status: 'ready' | 'phone_required' | 'preflight_unavailable' }
  | { status: 'unknown'; operation: string }
  | { status: 'denied'; operation: string; intercepted: boolean }
  | { status: 'delivered'; operation: string; messageId: string; row?: MessageRow }
);
export type MessageReadAllAction =
  | { kind: 'status' | 'prepare' }
  | { kind: 'confirm' | 'cancel'; intent: string }
  | { kind: 'check'; operation: string };
export type MessageReadAllView =
  | { status: 'idle' | 'unavailable' }
  | { status: 'ready'; intent: string }
  | {
      status:
        | 'pending'
        | 'uncertain'
        | 'confirmed'
        | 'not_sent'
        | 'checked_clear'
        | 'check_inconclusive'
        | 'check_unavailable';
      operation: string;
    };
export interface MessageClient {
  messageReadAll(
    epoch: string,
    action: MessageReadAllAction,
  ): Promise<AccountData<MessageReadAllView>>;
  messageReply(epoch: string, action: MessageReplyAction): Promise<AccountData<MessageReplyView>>;
  watchMessageCounts(update: (view: MessageCountView) => void): Promise<MessageCountStream>;
  messagePage(epoch: string, query: MessageQuery): Promise<AccountData<MessagePage>>;
  pendingMessageDeletions(epoch: string): Promise<AccountData<MessagePendingDeletion[]>>;
  deleteMessage(
    epoch: string,
    receipt: string,
    action: 'remove' | 'check',
  ): Promise<AccountData<MessageDeleteOutcome>>;
}
