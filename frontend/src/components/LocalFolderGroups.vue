<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { LocalPage } from '../services/local';
import { localFolderTree, visibleLocalFolders } from '../services/localFolders';
import Icon from './Icon.vue';

const props = defineProps<{
  groups: LocalPage['groups'];
  selected: string | null;
  searching: boolean;
}>();
const emit = defineEmits<{ select: [key: string] }>();
const expanded = defineModel<Set<string>>('expanded', { required: true });
const roots = computed(() => localFolderTree(props.groups));
const rows = computed(() => visibleLocalFolders(roots.value, expanded.value));
const treeElement = ref<HTMLElement>();
let revealed: string | undefined;
function toggle(key: string) {
  const next = new Set(expanded.value);
  if (!next.delete(key)) next.add(key);
  expanded.value = next;
}
function navigate(event: KeyboardEvent, index: number) {
  if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
  const node = rows.value[index]?.node;
  if (!node) return;
  let target: string | undefined;
  switch (event.key) {
    case 'ArrowRight':
      if (node.children.length && !expanded.value.has(node.group.key)) toggle(node.group.key);
      else target = node.children[0]?.group.key;
      break;
    case 'ArrowLeft':
      if (node.children.length && expanded.value.has(node.group.key)) toggle(node.group.key);
      else target = node.parent?.group.key;
      break;
    case 'ArrowUp':
      target = rows.value[index - 1]?.node.group.key;
      break;
    case 'ArrowDown':
      target = rows.value[index + 1]?.node.group.key;
      break;
    case 'Home':
      target = rows.value[0]?.node.group.key;
      break;
    case 'End':
      target = rows.value.at(-1)?.node.group.key;
      break;
    default:
      return;
  }
  event.preventDefault();
  if (target !== undefined)
    Array.from(
      treeElement.value?.querySelectorAll<HTMLButtonElement>('[data-local-folder-key]') ?? [],
    )
      .find((button) => button.dataset.localFolderKey === target)
      ?.focus();
}
watch(
  [() => props.selected, () => props.searching, roots],
  ([selected, searching, tree]) => {
    if (selected == null || searching) {
      revealed = undefined;
      return;
    }
    if (revealed === selected) return;
    revealed = undefined;
    const pending = [...tree];
    while (pending.length) {
      const node = pending.pop()!;
      if (node.group.key !== selected) {
        for (const child of node.children) pending.push(child);
        continue;
      }
      const next = new Set(expanded.value);
      for (let parent = node.parent; parent; parent = parent.parent) next.add(parent.group.key);
      expanded.value = next;
      revealed = selected;
      break;
    }
  },
  { immediate: true },
);
</script>

<template>
  <template v-if="searching">
    <button
      v-for="group in groups"
      :key="group.key"
      type="button"
      :aria-pressed="selected === group.key"
      :data-local-folder-key="group.key"
      :data-local-group-key="group.key"
      aria-haspopup="menu"
      aria-expanded="false"
      aria-keyshortcuts="Shift+F10"
      :title="group.name"
      @click="emit('select', group.key)"
    >
      <span>{{ group.name }}</span
      ><small>{{ group.count }}</small>
    </button>
  </template>
  <div v-else ref="treeElement" class="local-folder-rows" role="group" aria-label="文件夹层级">
    <div
      v-for="({ node, depth }, index) in rows"
      :key="node.group.key"
      class="local-folder-row"
      :style="{ '--folder-depth': Math.min(depth, 4) }"
    >
      <button
        v-if="node.children.length"
        type="button"
        class="local-folder-toggle"
        :aria-label="`${expanded.has(node.group.key) ? '折叠' : '展开'}：${node.group.name}`"
        :aria-expanded="expanded.has(node.group.key)"
        @click="toggle(node.group.key)"
      >
        <Icon name="down" />
      </button>
      <div v-else class="local-folder-spacer" aria-hidden="true" />
      <button
        type="button"
        class="local-folder-select"
        :aria-pressed="selected === node.group.key"
        :data-local-folder-key="node.group.key"
        :data-local-group-key="node.group.key"
        aria-haspopup="menu"
        aria-expanded="false"
        aria-keyshortcuts="Shift+F10"
        :title="node.group.name"
        @keydown="navigate($event, index)"
        @click="emit('select', node.group.key)"
      >
        <Icon name="folder" /><span>{{ node.name }}</span
        ><small>{{ node.group.count }}</small>
      </button>
    </div>
  </div>
</template>
