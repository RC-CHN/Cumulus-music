<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import type { ProfileRelationMode } from '../../services/profileRelations';
import type { usePlaylistCreator } from '../../composables/usePlaylistCreator';
import Icon from '../Icon.vue';
import ProfileFollowControl from '../profile/ProfileFollowControl.vue';
import ProfileBlacklistControl from '../profile/ProfileBlacklistControl.vue';
import type { ProfileBlacklistResult } from '../../services/profileBlacklist';
import type { useProfileFollow } from '../../composables/useProfileFollow';
import PlaylistCard from '../PlaylistCard.vue';

import { usePlaybackTriggerFocus } from '../../composables/usePlaybackTriggerFocus';

const props = defineProps<{
  model: ReturnType<typeof usePlaylistCreator>;
  profileFollow: ReturnType<typeof useProfileFollow>;
  checkingRelations: boolean;
  accountEpoch?: string;
  relationNotice: string;
  rankPlayable: boolean;
  rankTransport: 'playing' | 'paused' | null;
  activeSection: 'playlists' | 'notes' | 'podcasts';
  managingPodcasts?: boolean;
  showManagePodcasts?: boolean;
}>();
const emit = defineEmits<{
  open: [id: string];
  relations: [kind: ProfileRelationMode];
  rank: [];
  playRank: [];
  section: [value: 'playlists' | 'notes' | 'podcasts'];
  managePodcasts: [];
  blacklistUpdated: [uid: string, result: ProfileBlacklistResult];
  expired: [];
}>();
const root = ref<HTMLElement>();
const rankShell = ref<HTMLElement>();
let rankFocusTarget: string | undefined;
const rankFocus = usePlaybackTriggerFocus(
  () => props.rankPlayable,
  rankShell,
  () =>
    props.model.profile.value?.id === rankFocusTarget &&
    !rankShell.value?.closest('.page-leave-active'),
);
function playRankCard() {
  rankFocusTarget = props.model.profile.value?.id;
  rankFocus.start(() => emit('playRank'));
}
const failedAvatar = ref(false);
const followersCount = computed(
  () =>
    props.profileFollow.active.value?.relation?.followersCount ??
    props.model.profile.value?.followersCount,
);
const rankEntry = computed(() =>
  props.model.group.value === 'created' && props.model.page.value === 0
    ? props.model.profile.value?.listeningRank
    : undefined,
);

onMounted(async () => {
  const origin = document.activeElement;
  await nextTick();
  if (!root.value?.isConnected || root.value.closest('[inert]')) return;
  if (document.activeElement !== document.body && document.activeElement !== origin) return;
  if (props.activeSection !== 'playlists') {
    root.value
      .querySelector<HTMLElement>('.creator-page-tabs [aria-pressed="true"]')
      ?.focus({ preventScroll: true });
    return;
  }
  const card = Array.from(
    root.value.querySelectorAll<HTMLButtonElement>('[data-library-open]'),
  ).find((node) => node.dataset.libraryOpen === props.model.opened.value);
  const rank =
    props.model.openedRank.value &&
    root.value.querySelector<HTMLElement>('[data-profile-rank-entry]');
  (rank || relationButton() || card || root.value.querySelector('h1'))?.focus({
    preventScroll: true,
  });
});
function relationButton() {
  return root.value?.querySelector<HTMLElement>(
    `[data-relation-entry="${props.model.openedRelation.value ?? ''}"]`,
  );
}
watch(
  () => props.model.profile.value?.id,
  () => {
    if (document.activeElement === root.value?.querySelector('h1'))
      relationButton()?.focus({ preventScroll: true });
  },
  { flush: 'post' },
);
function relations(kind: ProfileRelationMode) {
  if (props.checkingRelations) return;
  props.model.openedRank.value = false;
  props.model.rememberRelation(kind);
  emit('relations', kind);
}
const count = new Intl.NumberFormat('zh-CN', { notation: 'compact' });
function open(id: string) {
  props.model.openedRank.value = false;
  props.model.opened.value = id;
  emit('open', id);
}
</script>

<template>
  <section
    ref="root"
    class="playlist-creator-page"
    aria-label="用户公开主页"
    :aria-busy="model.busy.value"
  >
    <header class="creator-page-header">
      <span v-if="model.profile.value" class="creator-page-portrait">
        <img
          v-if="model.profile.value.avatarUrl && !failedAvatar"
          :src="model.profile.value.avatarUrl"
          alt=""
          referrerpolicy="no-referrer"
          @error="failedAvatar = true"
        />
        <Icon v-else name="user" />
      </span>
      <div>
        <h1 tabindex="-1">{{ model.profile.value?.nickname ?? '用户主页' }}</h1>
        <p v-if="model.profile.value?.signature" class="muted">
          {{ model.profile.value.signature }}
        </p>
        <div
          v-if="model.profile.value"
          class="creator-relations"
          role="group"
          aria-label="关注和粉丝"
        >
          <button
            v-if="model.profile.value.followingCount"
            class="text-button"
            type="button"
            data-relation-entry="following"
            :aria-disabled="checkingRelations"
            @click="relations('following')"
          >
            关注 {{ count.format(model.profile.value.followingCount) }}
          </button>
          <button
            v-if="followersCount"
            class="text-button"
            type="button"
            data-relation-entry="followers"
            :aria-disabled="checkingRelations"
            @click="relations('followers')"
          >
            粉丝 {{ count.format(followersCount) }}
          </button>
        </div>
        <div class="creator-guest-actions">
          <ProfileFollowControl
            v-if="
              !model.profile.value?.blacklist?.blocked && !model.profile.value?.blacklist?.pending
            "
            :model="profileFollow"
          />
          <ProfileBlacklistControl
            :epoch="accountEpoch"
            :uid="model.profile.value?.id"
            :nickname="model.profile.value?.nickname"
            :header="model.profile.value?.blacklist"
            @updated="(id, result) => emit('blacklistUpdated', id, result)"
            @expired="emit('expired')"
          />
        </div>
        <p v-if="checkingRelations" role="status">正在确认查看权限…</p>
        <p v-if="relationNotice" role="alert" data-relation-notice>{{ relationNotice }}</p>
      </div>
    </header>
    <p v-if="model.busy.value" role="status">正在读取公开资料…</p>
    <p v-else-if="model.failed.value" role="alert">
      无法读取公开资料
      <button class="text-button" type="button" @click="model.load">重试</button>
    </p>
    <p v-else-if="model.profile.value === null" class="muted">用户不存在</p>
    <p v-else-if="model.profile.value?.playlistsVisible === false" role="status">
      此用户的歌单不可查看
    </p>
    <template v-else-if="model.profile.value">
      <nav
        v-if="model.profile.value.notesCount != null"
        class="creator-page-tabs"
        aria-label="用户主页分类"
      >
        <button
          class="text-button"
          type="button"
          :aria-pressed="activeSection === 'playlists'"
          @click="emit('section', 'playlists')"
        >
          歌单
        </button>
        <button
          class="text-button"
          type="button"
          :aria-pressed="activeSection === 'notes'"
          @click="emit('section', 'notes')"
        >
          笔记 {{ count.format(model.profile.value.notesCount) }}
        </button>
        <button
          v-if="model.profile.value.podcasts"
          class="text-button"
          type="button"
          :aria-pressed="activeSection === 'podcasts'"
          @click="emit('section', 'podcasts')"
        >
          播客 {{ count.format(model.profile.value.podcasts.total) }}
        </button>
        <button
          v-if="
            activeSection === 'podcasts' &&
            model.profile.value.podcasts?.isSelf &&
            showManagePodcasts
          "
          class="secondary creator-manage-podcasts"
          type="button"
          :disabled="managingPodcasts"
          title="在浏览器中登录当前账号并打开播客管理"
          @click="emit('managePodcasts')"
        >
          <Icon name="edit" />管理播客
        </button>
      </nav>
      <slot v-if="activeSection !== 'playlists'" />
      <template v-else>
        <div class="creator-page-groups" role="group" aria-label="用户歌单">
          <button
            class="text-button"
            type="button"
            :aria-pressed="model.group.value === 'created'"
            @click="model.select('created')"
          >
            创建的歌单 {{ model.profile.value.created.length }}
          </button>
          <button
            class="text-button"
            type="button"
            :aria-pressed="model.group.value === 'collected'"
            @click="model.select('collected')"
          >
            收藏的歌单 {{ model.profile.value.collected.length }}
          </button>
        </div>
        <div v-if="rankEntry || model.cards.value.length" class="playlist-grid">
          <div
            v-if="rankEntry"
            ref="rankShell"
            class="playlist-shell creator-rank-shell"
            :class="{ 'library-current': rankTransport !== null }"
          >
            <button
              class="playlist-card creator-rank-entry"
              type="button"
              data-profile-rank-entry
              @click="
                model.openedRank.value = true;
                emit('rank');
              "
            >
              <span class="card-cover creator-rank-cover" aria-hidden="true">
                <Icon name="list" />
              </span>
              <span class="library-card-copy">
                <span class="card-title">{{
                  rankEntry.isSelf ? '我的听歌排行' : 'TA的听歌排行'
                }}</span>
                <small v-if="rankEntry.listenSongs !== null"
                  >累计听歌{{ rankEntry.listenSongs }}首</small
                >
              </span>
            </button>
            <button
              class="library-card-play"
              type="button"
              data-profile-rank-play
              :aria-label="`${rankTransport === 'playing' ? '暂停' : rankTransport === 'paused' ? '继续播放' : '播放'}${rankEntry.isSelf ? '我的' : 'TA的'}最近一周听歌排行`"
              :disabled="!rankPlayable"
              @click="playRankCard"
            >
              <Icon :name="rankTransport === 'playing' ? 'pause' : 'play'" />
            </button>
          </div>
          <PlaylistCard
            v-for="list in model.cards.value"
            :key="list.id"
            :playlist="list"
            :items="[]"
            open-only
            @open="open(list.id)"
          />
        </div>
        <p v-else class="muted">暂无公开歌单</p>
        <nav v-if="model.pages.value > 1" aria-label="用户歌单分页">
          <button
            class="text-button"
            type="button"
            :disabled="model.page.value === 0"
            @click="model.page.value--"
          >
            上一页
          </button>
          <button
            class="text-button"
            type="button"
            :disabled="model.page.value + 1 >= model.pages.value"
            @click="model.page.value++"
          >
            下一页
          </button>
        </nav>
      </template>
    </template>
  </section>
</template>

<style scoped>
.creator-guest-actions {
  display: flex;
  align-items: baseline;
  gap: 10px;
  flex-wrap: wrap;
}
.creator-page-header {
  display: flex;
  gap: 20px;
  align-items: center;
  overflow-wrap: anywhere;
  margin-bottom: 20px;
}
.creator-page-header h1 {
  margin: 0;
}
.creator-page-portrait {
  display: grid;
  flex: 0 0 80px;
  height: 80px;
  place-items: center;
  overflow: hidden;
  border-radius: 50%;
  background: var(--surface-alt);
}
.creator-page-portrait img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.creator-relations {
  display: flex;
  gap: 12px;
  margin-top: 8px;
}
.creator-page-tabs {
  display: flex;
  gap: 18px;
  margin-bottom: 14px;
  border-bottom: 1px solid var(--line);
}
.creator-manage-podcasts {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin-inline-start: auto;
  white-space: nowrap;
}
.creator-manage-podcasts :deep(svg) {
  width: 16px;
  height: 16px;
}
.creator-page-tabs [aria-pressed='true'] {
  border-bottom: 2px solid var(--accent);
  font-weight: 600;
}
.creator-page-groups {
  display: flex;
  gap: 12px;
  margin-bottom: 12px;
}
.creator-page-groups [aria-pressed='true'] {
  font-weight: 600;
  background: var(--surface-alt);
}
.creator-rank-entry {
  align-self: start;
}
.creator-rank-cover {
  color: var(--accent);
}
.creator-rank-cover .icon {
  width: 34%;
  height: 34%;
}
.creator-rank-shell {
  position: relative;
  align-self: start;
}
</style>
