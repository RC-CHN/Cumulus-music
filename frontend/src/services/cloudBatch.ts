import type { CloudClient } from './cloud';
import { readCloudCollection, type CloudCollectionData } from './cloudCollection.ts';
export type CloudBatchData = CloudCollectionData;
/** Official independent batch source is the complete natural cloudOnlineList. */
export function readCloudBatch(
  client: Pick<CloudClient, 'cloudPage'>,
  epoch: string,
  active: () => boolean,
  progress: (loaded: number, total: number) => void,
): Promise<CloudBatchData> {
  return readCloudCollection(
    client,
    epoch,
    { tab: 'uploaded', keyword: '', sort: 'natural', descending: false, offset: 0, limit: 200 },
    active,
    progress,
  );
}
