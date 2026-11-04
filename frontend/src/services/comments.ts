import type { AccountData, LikeResult } from './contracts';
import type { InjectionKey } from 'vue';
export interface CommentResource {
  kind: 'playlist' | 'voice' | 'event' | 'album' | 'track' | 'mv' | 'topic' | 'video';
  id: string;
  owner?: string | null;
}
export interface CommentQuery {
  sort: 'latest' | 'hot';
  limit: number;
  offset: number;
  beforeTime: number;
  commentId: string | null;
}
export type CommentReadNavigation = 'refresh' | 'next' | 'back' | 'reset';
export interface CommentReadRetry<Query> {
  query: Query;
  navigation: CommentReadNavigation;
}
export interface CommentAuthor {
  name: string;
  avatarUrl: string | null;
  profileId?: string | null;
}
export interface CommentAuthorAnchor {
  scope: string;
  id: string;
  control: string;
  group: string;
  scrollTop: number;
}
export const commentAuthorNavigation: InjectionKey<{
  open: (epoch: string, id: string, anchor: CommentAuthorAnchor) => void;
  restore: (epoch: string, scope: string) => CommentAuthorAnchor | undefined;
  restored: (anchor: CommentAuthorAnchor) => void;
}> = Symbol('comment-author-navigation');
export interface CommentReply {
  id: string | null;
  content: string | null;
  author: CommentAuthor | null;
}
export interface CommentItem {
  id: string;
  parentId?: string | null;
  content: string;
  time: number;
  timeText: string;
  liked: boolean | null;
  likedCount: number;
  author: CommentAuthor | null;
  replies: CommentReply[];
  owned: boolean;
}
export interface CommentOperation {
  id: string;
  desired: boolean;
  phase: LikeResult['phase'];
  observed: CommentItem | null;
}
export interface CommentPage {
  resource: CommentResource;
  comments: CommentItem[];
  hotComments: CommentItem[];
  topComments: CommentItem[];
  currentComment: CommentItem | null;
  total: number;
  more: boolean;
  moreHot: boolean;
  next: CommentQuery | null;
  cached: boolean;
  operations: CommentOperation[];
}
export interface CommentClient {
  commentsPage(
    epoch: string,
    resource: CommentResource,
    query: CommentQuery,
    refresh: boolean,
  ): Promise<AccountData<CommentPage>>;
  commentsLike(
    epoch: string,
    resource: CommentResource,
    id: string,
    desired: boolean,
  ): Promise<AccountData<CommentOperation>>;
  commentsReconcile(
    epoch: string,
    resource: CommentResource,
    id: string,
  ): Promise<AccountData<CommentOperation>>;
}
export function commentQuery(sort: 'latest' | 'hot' = 'latest'): CommentQuery {
  return { sort, limit: 20, offset: 0, beforeTime: 0, commentId: null };
}
export function resourceKey(resource: CommentResource): string {
  return JSON.stringify([resource.kind, resource.id, resource.owner ?? null]);
}

/** Open the exact native comment in the first read without a list preflight. */
export function commentStartPosition(commentId?: string) {
  return commentId
    ? { query: { ...commentQuery(), commentId }, history: [] as CommentQuery[], focusTarget: true }
    : undefined;
}
