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
}

export interface CommunityFeedPage {
  cards: CommunityCard[];
  continuation: string | null;
  replace: boolean;
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

export interface CommunityClient {
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
