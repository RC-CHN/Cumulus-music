<script setup lang="ts">
import { ref, watch } from 'vue';
import type { FollowCard } from '../../services/collections';
import { artistCardMenu, type ArtistCardRequest } from '../../services/artistCards';
import ActionMenu from '../ActionMenu.vue';
import Icon from '../Icon.vue';
const props = defineProps<{ entry: FollowCard }>();
const emit = defineEmits<{ artist: [id: string]; action: [request: ArtistCardRequest] }>();
const element = ref<HTMLElement>(),
  menu = ref<InstanceType<typeof ActionMenu>>();
const failed = ref(false);
watch(
  () => props.entry.user?.avatarUrl ?? props.entry.artist?.coverUrl,
  () => {
    failed.value = false;
  },
);
const items = artistCardMenu(true);
function action(key: string) {
  const artist = props.entry.artist;
  if (!artist) return;
  if (key === 'open') emit('artist', artist.id);
  else if (key === 'copy')
    emit('action', {
      artist,
      action: key,
      anchor: document.activeElement instanceof HTMLElement ? document.activeElement : null,
    });
}
function context(event: MouseEvent | KeyboardEvent) {
  if (!props.entry.artist) return;
  const anchor =
    (event.target instanceof Element ? event.target.closest<HTMLButtonElement>('button') : null) ??
    element.value?.querySelector<HTMLButtonElement>('.collection-follow-open');
  if (!anchor) return;
  const box = anchor.getBoundingClientRect(),
    pointer = event instanceof MouseEvent && event.button === 2;
  void menu.value?.showAt(
    pointer ? event.clientX : box.left,
    pointer ? event.clientY : box.bottom,
    anchor,
  );
}
function keyboard(event: KeyboardEvent) {
  if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
    event.preventDefault();
    context(event);
  }
}
</script>
<template>
  <article
    ref="element"
    class="collection-follow-card"
    @contextmenu.prevent="context"
    @keydown="keyboard"
  >
    <button
      v-if="entry.artist"
      class="collection-follow-open"
      :data-collection-key="`artist:${entry.artist.id}`"
      :aria-label="`查看歌手 ${entry.artist.name}`"
      @click="emit('artist', entry.artist.id)"
    />
    <img
      v-if="!failed && (entry.user?.avatarUrl || entry.artist?.coverUrl)"
      :src="entry.user?.avatarUrl ?? entry.artist?.coverUrl ?? undefined"
      alt=""
      loading="lazy"
      decoding="async"
      referrerpolicy="no-referrer"
      @error="failed = true"
    />
    <span v-else class="collection-avatar"><Icon name="user" /></span>
    <span class="collection-follow-info"
      ><strong :title="entry.artist?.name || entry.user?.name">{{
        entry.artist?.name || entry.user?.name || '资料暂缺'
      }}</strong>
      <small v-if="entry.user?.unavailable">关联用户已注销</small>
      <small v-else-if="entry.user?.signature">{{ entry.user.signature }}</small>
      <small v-if="!entry.artist && entry.user">用户主页暂未接入</small>
    </span>
    <ActionMenu
      v-if="entry.artist"
      ref="menu"
      class="collection-follow-menu"
      variant="song"
      :label="`更多关注歌手操作：${entry.artist.name}`"
      :catalog-key="`artist:${entry.artist.id}:menu`"
      :items="items"
      @select="action"
    />
  </article>
</template>
