import type { AccountData } from './contracts';

export type CommunityFeedAction =
  | { kind: 'first' | 'refresh' }
  | { kind: 'next'; continuation: string };

export type CommunityResource = {
  type: 'resource';
  kind: 'track' | 'playlist' | 'album' | 'artist' | 'podcast' | 'voice';
  id: string;
  label: string;
  coverUrl: string | null;
};

export type CommunityAttachment =
  | CommunityResource
  | { type: 'forwarded'; card: CommunityCard }
  | { type: 'unavailable'; label: string };

export interface CommunityCard {
  key: string;
  authorName: string;
  avatarUrl: string | null;
  eventTime: number | null;
  timeText: string | null;
  title: string | null;
  message: string | null;
  pictures: string[];
  attachment: CommunityAttachment | null;
  counters: { likes: number | null; shares: number | null; comments: number | null };
  status: 'ready' | 'unsupported' | 'deleted' | 'unavailable';
  detail: string | null;
  canDelete?: boolean;
}

export type CommunityDeleteAction =
  | { kind: 'inspect' | 'check' }
  | { kind: 'confirm'; confirmation: string };
export interface CommunityDeleteReply {
  status:
    | 'ready'
    | 'not_sent'
    | 'uncertain'
    | 'still_visible'
    | 'unavailable'
    | 'acknowledged'
    | 'observed_removed'
    | 'rejected';
  retired: string[];
  confirmation?: string;
}
export interface CommunityDeleteClient {
  communityNoteDelete(
    epoch: string,
    detail: string,
    action: CommunityDeleteAction,
  ): Promise<AccountData<CommunityDeleteReply>>;
}

export interface CommunityFeedPage {
  cards: CommunityCard[];
  continuation: string | null;
  replace: boolean;
  unavailable?: boolean;
}

export interface CommunityDetail {
  card: CommunityCard;
  commentsAvailable: boolean;
}

export interface CommunityComment {
  id: string;
  authorName: string | null;
  text: string;
  time: number;
}

export interface CommunityCommentPage {
  comments: CommunityComment[];
  hotComments: CommunityComment[];
  nextOffset: number | null;
  total: number;
}

export type ComposerKind = 'song' | 'artist' | 'album' | 'playlist' | 'podcast';
export interface ComposerAttachment {
  kind: ComposerKind;
  id: string;
  title: string;
  subtitle: string | null;
  coverUrl: string | null;
}
export interface ComposerSearchItem extends ComposerAttachment {
  receipt: string;
}
export interface ComposerDraft {
  revision: string;
  text: string;
  attachment: ComposerAttachment | null;
}
export type ComposerAttachmentEdit =
  | { kind: 'keep' | 'remove' }
  | { kind: 'select'; receipt: string };
export type ComposerDraftAction =
  | { kind: 'read' }
  | { kind: 'save'; revision: string | null; text: string; attachment: ComposerAttachmentEdit }
  | { kind: 'discard'; revision: string };
export type ComposerPublishAction =
  | { kind: 'status' }
  | { kind: 'check_own_notes' }
  | { kind: 'submit'; revision: string };
export type ComposerPublication =
  | { status: 'ready' | 'phone_required' | 'preflight_unavailable' }
  | { status: 'uncertain'; revision: string }
  | { status: 'removed'; revision: string }
  | {
      status: 'rejected';
      revision: string;
      reason: 'cannot_share' | 'private_cloud' | 'purchase_required' | 'not_sent';
    }
  | { status: 'published'; revision: string; eventId: string; detail: string };
export interface ProfileNotesClient {
  profileNotes(
    epoch: string,
    uid: string,
    action: CommunityFeedAction,
  ): Promise<AccountData<CommunityFeedPage>>;
}

export interface CommunityClient {
  communityComposerSearch(
    epoch: string,
    kind: ComposerKind,
    keyword: string,
    offset: number,
  ): Promise<AccountData<{ items: ComposerSearchItem[]; nextOffset: number | null }>>;
  communityComposerDraft(
    epoch: string,
    action: ComposerDraftAction,
  ): Promise<AccountData<ComposerDraft | null>>;
  communityComposerPublish(
    epoch: string,
    action: ComposerPublishAction,
  ): Promise<AccountData<ComposerPublication>>;
  communityFeed(
    epoch: string,
    action: CommunityFeedAction,
  ): Promise<AccountData<CommunityFeedPage>>;
  communityDetail(epoch: string, detail: string): Promise<AccountData<CommunityDetail | null>>;
  communityComments(
    epoch: string,
    detail: string,
    offset: number,
  ): Promise<AccountData<CommunityCommentPage>>;
}

export interface CommunityNoticeCard {
  style: 'artists' | 'musicians';
  text: string;
  avatars: string[];
  cover: string | null;
}

export interface CommunityNoticeClient {
  communityNewSongNotice(
    epoch: string,
    mode: 'badge' | 'visit',
  ): Promise<AccountData<{ showDot: boolean; card: CommunityNoticeCard | null }>>;
}
