import type { AccountData } from './contracts';
import type { CommentItem, CommentResource } from './comments';
export interface CommentFloorQuery {
  parentId: string;
  time: number;
  limit: number;
}
export interface CommentFloorPage {
  resource: CommentResource;
  query: CommentFloorQuery;
  owner: CommentItem | null;
  currentComment: CommentItem | null;
  comments: CommentItem[];
  bestComments: CommentItem[];
  total: number;
  more: boolean;
  next: CommentFloorQuery | null;
  cached: boolean;
}
export interface CommentFloorClient {
  commentsFloor(
    epoch: string,
    resource: CommentResource,
    query: CommentFloorQuery,
    refresh: boolean,
  ): Promise<AccountData<CommentFloorPage>>;
}
