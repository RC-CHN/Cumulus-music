import type { AccountData, CoreFailure } from '../services/contracts.ts';
import type { PodcastLikedClient, PodcastLikedQuery } from '../services/podcast.ts';

type Invoke = <T>(command: string, payload: Record<string, unknown>) => Promise<T>;

function likedQuery(query: PodcastLikedQuery): PodcastLikedQuery {
  const { kind } = query;
  if (kind === 'open') return { kind };
  const { visit, revision } = query;
  return kind === 'next'
    ? { kind, visit, revision, continuation: query.continuation }
    : { kind, visit, revision };
}

export function createPodcastLikedBridge(invoke: Invoke): PodcastLikedClient {
  async function read<T>(command: string, epoch: string, args: Record<string, unknown> = {}) {
    const reply = await invoke<AccountData<T>>(command, { epoch, ...args });
    if (reply.epoch !== epoch) {
      throw {
        code: 'stale',
        message: '操作已取消或账号已切换',
        retryable: false,
      } satisfies CoreFailure;
    }
    return reply;
  }

  return {
    podcastLikedPage: (epoch, query) =>
      read('podcast_liked_page', epoch, { query: likedQuery(query) }),
    podcastLikedCount: (epoch) => read('podcast_liked_count', epoch),
    podcastLikedPageProgram: (epoch, { visit, revision, voiceId }) =>
      read('podcast_liked_page_program', epoch, { target: { visit, revision, voiceId } }),
  };
}
