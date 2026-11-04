<script setup lang="ts">
import { nextTick, onMounted, ref } from 'vue';
import Icon from '../Icon.vue';
import {
  PROFILE_PODCAST_GROUPS,
  PROFILE_PODCAST_PAGE_SIZE,
  type ProfilePodcastGroup,
  type useProfilePodcasts,
} from '../../composables/useProfilePodcasts';

const props = defineProps<{ model: ReturnType<typeof useProfilePodcasts>; isSelf: boolean }>();
const emit = defineEmits<{ open: [id: string] }>();
const root = ref<HTMLElement>();
const count = new Intl.NumberFormat('zh-CN', { notation: 'compact' });
const titles = { created: '创建的播客', collected: '收藏的播客' } as const;
const pageSize = PROFILE_PODCAST_PAGE_SIZE;

function rows(kind: ProfilePodcastGroup) {
  const state = props.model.groups[kind];
  const items = state.page?.rows ?? [];
  return kind === 'created' ? items : items.slice(state.localOffset, state.localOffset + pageSize);
}

function open(kind: ProfilePodcastGroup, id: string) {
  props.model.opener.value = { group: kind, id };
  emit('open', id);
}

onMounted(async () => {
  await nextTick();
  const origin = props.model.opener.value;
  if (!origin || !root.value?.isConnected) return;
  const opener = Array.from(
    root.value.querySelectorAll<HTMLButtonElement>('[data-profile-podcast]'),
  ).find(
    (node) =>
      node.dataset.profilePodcast === origin.id &&
      node.dataset.profilePodcastGroup === origin.group,
  );
  if (opener) {
    opener.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    opener.focus({ preventScroll: true });
    props.model.opener.value = null;
  }
});

async function turn(kind: ProfilePodcastGroup, offset: number) {
  if (kind === 'created') await props.model.read(kind, offset);
  else props.model.groups[kind].localOffset = offset;
  await nextTick();
  const heading = root.value?.querySelector<HTMLElement>(
    `[data-profile-podcast-heading="${kind}"]`,
  );
  heading?.scrollIntoView({ block: 'start', behavior: 'instant' });
  heading?.focus({ preventScroll: true });
}
</script>

<template>
  <section ref="root" class="profile-podcasts" aria-label="用户播客">
    <p v-if="model.unavailable.value" role="status">此用户的播客不可查看</p>
    <template v-else>
      <p v-if="model.managementIssue.value" role="alert">{{ model.managementIssue.value }}</p>
      <section
        v-for="kind in PROFILE_PODCAST_GROUPS"
        :key="kind"
        class="profile-podcast-group"
        :aria-label="(isSelf ? '我' : 'Ta') + titles[kind]"
        :aria-busy="model.groups[kind].pending"
      >
        <header class="profile-podcast-heading">
          <h2 :data-profile-podcast-heading="kind" tabindex="-1">
            {{ (isSelf ? '我' : 'Ta') + titles[kind] }}
          </h2>
          <div class="profile-podcast-modes" role="group" :aria-label="titles[kind] + '显示方式'">
            <button
              v-for="mode in ['grid', 'list'] as const"
              :key="mode"
              class="icon-button"
              type="button"
              :aria-label="mode === 'grid' ? '网格显示' : '列表显示'"
              :title="mode === 'grid' ? '网格显示' : '列表显示'"
              :aria-pressed="model.groups[kind].display === mode"
              @click="model.groups[kind].display = mode"
            >
              <Icon :name="mode === 'grid' ? 'grid' : 'list'" />
            </button>
          </div>
        </header>
        <p v-if="model.groups[kind].pending && !model.groups[kind].page" role="status">
          正在读取播客…
        </p>
        <p v-else-if="model.groups[kind].issue" role="alert">
          {{ model.groups[kind].issue }}
          <button
            class="text-button"
            type="button"
            @click="model.read(kind, model.groups[kind].requested)"
          >
            重试
          </button>
        </p>
        <p v-else-if="model.groups[kind].page && !rows(kind).length" class="muted">暂无播客</p>
        <div
          v-if="rows(kind).length"
          :class="model.groups[kind].display === 'grid' ? 'podcast-grid' : 'profile-podcast-list'"
        >
          <button
            v-for="row in rows(kind)"
            :key="row.card.id"
            class="podcast-card"
            type="button"
            :data-profile-podcast="row.card.id"
            :data-profile-podcast-group="kind"
            :disabled="row.card.unavailable"
            @click="open(kind, row.card.id)"
          >
            <img
              v-if="row.card.coverUrl"
              :src="row.card.coverUrl"
              alt=""
              loading="lazy"
              referrerpolicy="no-referrer"
            />
            <span v-else class="podcast-art"><Icon name="music" /></span>
            <strong>{{ row.card.name }}</strong>
            <span v-if="kind === 'collected' && row.card.creator" class="profile-podcast-creator">
              {{ row.card.creator }}
            </span>
            <small v-if="model.groups[kind].display === 'list'">
              {{ count.format(row.subscribers) }} 人收藏
            </small>
          </button>
        </div>
        <template v-if="model.groups[kind].page">
          <nav
            v-if="
              kind === 'created'
                ? (model.groups.created.page?.offset ?? 0) > 0 ||
                  model.groups.created.page?.nextOffset != null
                : (model.groups.collected.page?.rows.length ?? 0) > pageSize
            "
            class="profile-podcast-pagination"
            :aria-label="titles[kind] + '分页'"
          >
            <button
              class="text-button"
              type="button"
              :disabled="
                model.groups[kind].pending ||
                (kind === 'created'
                  ? !model.groups.created.page?.offset
                  : !model.groups.collected.localOffset)
              "
              @click="
                turn(
                  kind,
                  kind === 'created'
                    ? (model.groups.created.page?.offset ?? pageSize) - pageSize
                    : model.groups.collected.localOffset - pageSize,
                )
              "
            >
              上一页
            </button>
            <button
              class="text-button"
              type="button"
              :disabled="
                model.groups[kind].pending ||
                (kind === 'created'
                  ? model.groups.created.page?.nextOffset == null
                  : model.groups.collected.localOffset + pageSize >=
                    (model.groups.collected.page?.rows.length ?? 0))
              "
              @click="
                turn(
                  kind,
                  kind === 'created'
                    ? (model.groups.created.page?.nextOffset ?? 0)
                    : model.groups.collected.localOffset + pageSize,
                )
              "
            >
              下一页
            </button>
          </nav>
          <p v-if="model.groups[kind].page?.truncated" class="muted">仅显示服务器返回的播客。</p>
        </template>
      </section>
    </template>
  </section>
</template>

<style scoped>
.profile-podcast-group + .profile-podcast-group {
  margin-top: 38px;
}
.profile-podcast-heading,
.profile-podcast-modes,
.profile-podcast-pagination {
  display: flex;
  align-items: center;
}
.profile-podcast-heading {
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
}
.profile-podcast-heading h2 {
  margin: 0;
  font-size: 22px;
}
.profile-podcast-modes,
.profile-podcast-pagination {
  gap: 5px;
}
.profile-podcast-pagination {
  margin-top: 12px;
}
.profile-podcast-modes [aria-pressed='true'] {
  color: var(--accent);
}
.profile-podcast-list .podcast-card {
  display: grid;
  width: 100%;
  grid-template-columns: 42px minmax(0, 2fr) minmax(0, 1fr) auto;
  align-items: center;
  padding: 7px;
}
.profile-podcast-list .podcast-card > img,
.profile-podcast-list .podcast-art {
  width: 40px;
  height: 40px;
}
.profile-podcast-list .podcast-art :deep(svg) {
  width: 24px;
  height: 24px;
}
.profile-podcast-list small {
  grid-column: 4;
}
</style>
