import { inject, type InjectionKey } from 'vue';
import type { CoreClient } from './contracts';

export const coreClientKey: InjectionKey<CoreClient> = Symbol('core-client');

export function useCore(): CoreClient {
  const client = inject(coreClientKey);
  if (!client) throw new Error('CoreClient has not been provided');
  return client;
}
