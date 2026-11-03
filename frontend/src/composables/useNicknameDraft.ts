import { onScopeDispose, ref, watch } from 'vue';
import { usableNicknameRecommendation } from '../services/nickname.ts';

export interface NicknameDraftContext {
  owner: string;
  suggestion: string | null | undefined;
}
interface RecommendationRequest {
  owner: string;
  revision: number;
}

export function useNicknameDraft(context: () => NicknameDraftContext | null, scrub: () => void) {
  const name = ref('');
  const edited = ref(false);
  const recommendationError = ref('');
  let owner: string | null = null;
  let revision = 0;
  let disposed = false;
  function clear() {
    revision++;
    name.value = '';
    edited.value = false;
    recommendationError.value = '';
    scrub();
  }
  const stop = watch(
    context,
    (current) => {
      const nextOwner = current?.owner ?? null;
      if (nextOwner === owner) return;
      clear();
      owner = nextOwner;
      if (current && usableNicknameRecommendation(current.suggestion))
        name.value = current.suggestion;
    },
    { immediate: true, flush: 'sync' },
  );
  function edit(value: string) {
    if (disposed || !owner) return;
    revision++;
    name.value = value;
    edited.value = true;
    recommendationError.value = '';
  }
  function recommendation(): RecommendationRequest | null {
    return !disposed && owner ? { owner, revision } : null;
  }
  function acceptRecommendation(
    request: RecommendationRequest | null,
    value: string | null | undefined,
  ): boolean {
    if (disposed || !request || request.owner !== owner || request.revision !== revision)
      return false;
    if (!usableNicknameRecommendation(value)) {
      recommendationError.value = '暂时无法获取推荐昵称，可手动填写';
      return false;
    }
    revision++;
    name.value = value;
    edited.value = true;
    recommendationError.value = '';
    return true;
  }
  function submitted() {
    revision++;
    edited.value = false;
    recommendationError.value = '';
  }
  onScopeDispose(() => {
    disposed = true;
    owner = null;
    stop();
    clear();
  });
  return {
    name,
    edited,
    recommendationError,
    edit,
    clear,
    recommendation,
    acceptRecommendation,
    submitted,
  };
}
