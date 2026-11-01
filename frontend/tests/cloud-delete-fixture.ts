import { effectScope } from 'vue';
import { useCloud } from '../src/composables/useCloud.ts';
import { useCloudDelete } from '../src/composables/useCloudDelete.ts';
import type { CloudClient, CloudPage, CloudRemoval } from '../src/services/cloud.ts';
export function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
export async function fixture() {
  let revision = 'r1',
    ended = 0;
  const calls: any[] = [];
  let receipt: CloudRemoval = {
    epoch: '1',
    id: 'ours',
    ids: ['7'],
    titles: ['Original'],
    kinds: ['private'],
    revision: 'r1',
    phase: 'prepared',
    running: false,
    deleteLocal: null,
    localCount: 1,
    cueCount: 0,
    localIssue: null,
    files: [{ cloudId: '7', title: 'Original', state: 'pending', issue: null }],
    deletion: null,
    legacyOnly: false,
    canContinue: false,
    error: null,
  };
  let execute = async (): Promise<CloudRemoval> => ({
    ...receipt,
    phase: 'result',
    deleteLocal: false,
    files: [],
    deletion: { id: 'ours', ids: ['7'], phase: 'rejected', total: 1, removed: 0, remaining: 1 },
  });
  const api = {
    cloudPage: async (e: string) =>
      ({
        epoch: e,
        revision,
        updatedMs: 1,
        size: 1,
        maxSize: 100,
        uploadedCount: 1,
        uploadingCount: 0,
        total: 1,
        offset: 0,
        tracks: [],
        uploads: [],
        deletions: [],
        workerError: null,
      }) satisfies CloudPage,
    cloudDeletePrepare: async (e: string, ids: string[], r: string) => {
      calls.push(['prepare', e, ids, r]);
      return receipt;
    },
    cloudDeleteExecute: async (e: string, id: string, local: boolean, retry: boolean) => {
      calls.push(['execute', e, id, local, retry]);
      return execute();
    },
    cloudDeleteView: async (e: string, id: string, reconcile: boolean) => {
      calls.push(['read', e, id, reconcile]);
      return receipt;
    },
    cloudDeleteDiscard: async (e: string, id: string) => {
      calls.push(['discard', e, id]);
    },
  };
  const model = useCloud(api as unknown as CloudClient);
  await model.connect({ epoch: '1', status: 'authenticated', nickname: null, avatarUrl: null });
  await model.refresh();
  const scope = effectScope();
  const input = {
    epoch: '1',
    ids: ['7'],
    titles: ['Original'],
    kinds: ['private' as const],
    revision: 'r1',
  };
  const state = scope.run(() => useCloudDelete(model, input, () => ended++))!;
  return {
    model,
    state,
    scope,
    calls,
    input,
    api,
    get ended() {
      return ended;
    },
    receipt(v: Partial<CloudRemoval>) {
      receipt = { ...receipt, ...v };
    },
    revision(v: string) {
      revision = v;
    },
    execute(v: () => Promise<CloudRemoval>) {
      execute = v;
    },
    dispose() {
      scope.stop();
      model.dispose();
    },
  };
}
