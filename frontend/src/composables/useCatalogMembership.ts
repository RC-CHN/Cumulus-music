import { computed, ref, shallowRef } from 'vue';
import { failure, type AccountView } from '../services/contracts.ts';
import type {
  CatalogClient,
  CatalogResource,
  CatalogMembershipAction,
  CatalogMembershipOperation,
} from '../services/catalog.ts';
import { useCore } from '../services/core.ts';

export function useCatalogMembership(
  core: Pick<CatalogClient, 'catalogMembership'> = useCore(),
  expired = () => {},
) {
  const operation = shallowRef<CatalogMembershipOperation | null>(null);
  const busy = ref(false),
    error = ref(''),
    known = ref(false);
  const pending = computed(
    () =>
      !!operation.value && ['sending', 'verifying', 'uncertain'].includes(operation.value.phase),
  );
  let epoch: string | undefined,
    target: CatalogResource | undefined,
    serial = 0,
    disposed = false;
  function connect(account: AccountView, resource: CatalogResource) {
    const next = account.status === 'authenticated' ? account.epoch : undefined;
    if (next === epoch && target?.kind === resource.kind && target.id === resource.id) return;
    epoch = next;
    target = { ...resource };
    serial++;
    operation.value = null;
    known.value = false;
    busy.value = false;
    error.value = '';
  }
  async function command(action: CatalogMembershipAction): Promise<boolean | null> {
    if (!epoch || !target || disposed || busy.value) return null;
    if (action.kind === 'set' && (!known.value || pending.value)) return null;
    const current = ++serial,
      owner = epoch,
      resource = { ...target };
    const active = () => !disposed && current === serial && epoch === owner;
    busy.value = true;
    error.value = '';
    const read = async (command: CatalogMembershipAction) => {
      const result = await core.catalogMembership(owner, resource, command);
      if (!active()) return null;
      if (
        result.epoch !== owner ||
        result.value.resource.id !== resource.id ||
        result.value.resource.kind !== resource.kind
      )
        throw { code: 'stale', message: '收藏状态响应已过期，请重试' };
      operation.value = result.value.operation;
      known.value = true;
      return result.value.operation;
    };
    try {
      const result = await read(action);
      return action.kind !== 'inspect' &&
        result &&
        ['confirmed', 'unchanged'].includes(result.phase)
        ? result.observed
        : null;
    } catch (e) {
      if (!active()) return null;
      const issue = failure(e);
      error.value = issue.message;
      if (['session_expired', 'profile_changed'].includes(issue.code)) {
        operation.value = null;
        known.value = false;
        expired();
      } else if (action.kind !== 'inspect') {
        known.value = false;
        // Inspect the durable journal after a failed write. Never retry it.
        try {
          await read({ kind: 'inspect' });
        } catch {
          /* keep writes disabled until a successful inspection */
        }
      }
      return null;
    } finally {
      if (active()) busy.value = false;
    }
  }
  function dispose() {
    disposed = true;
    serial++;
    operation.value = null;
    busy.value = false;
    known.value = false;
  }
  return { operation, busy, error, known, pending, connect, command, dispose };
}
