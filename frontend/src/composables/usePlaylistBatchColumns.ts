import { computed, type Ref } from 'vue';
import { useTableColumns } from './useTableColumns';
import { batchTableColumns } from '../services/playlistBatchTable';

/** Keep the existing batch boundary indices while sharing the drag lifecycle. */
export function usePlaylistBatchColumns(
  container: Ref<HTMLElement | undefined>,
  disabled: () => boolean,
) {
  const model = useTableColumns(container, () => batchTableColumns, disabled);
  return {
    ...model,
    dragging: computed(() =>
      model.dragging.value === undefined ? undefined : model.dragging.value - 1,
    ),
    minimum: (column: number) => model.minimum(column + 1),
    maximum: (column: number) => model.maximum(column + 1),
    value: (column: number) => model.value(column + 1),
    key: (event: KeyboardEvent, column: number) => model.key(event, column + 1),
    start: (event: PointerEvent, column: number) => model.start(event, column + 1),
  };
}
