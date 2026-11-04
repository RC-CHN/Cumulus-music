<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { failure, type AccountView } from '../services/contracts';
import { useCore } from '../services/core';
import { useAppearance } from '../composables/useAppearance';
import { useEffectRegistrationChanges } from '../composables/useEffectRegistrationChanges';
import type {
  EffectCatalogueItem,
  EffectCatalogueKind,
  EffectAccess,
  PendingEffectRegistration,
} from '../services/effectDirectory';
import type { BoundParametricStatus, DeviceParametricView } from '../services/player';
import Icon from './Icon.vue';
import EffectDeviceDirectory from './EffectDeviceDirectory.vue';
import EffectParametricControls from './EffectParametricControls.vue';

const props = defineProps<{
  account: AccountView;
  initialManagementOnly?: boolean;
  headphoneManagement?: boolean;
  bindingStatus?: BoundParametricStatus;
  deviceParametric?: DeviceParametricView;
  parametricProcessing?: boolean;
}>();
const emit = defineEmits<{ close: []; login: []; equalizer: [] }>();
const core = useCore();
const { effective: appearance } = useAppearance();
const failedArtwork = ref(new Set<string>());
const dialog = ref<HTMLDialogElement>();
const categories = [
  ['highly_recommended', '强推荐'],
  ['recommended', '官方精选音效'],
  ['more', '更多音效'],
  ['devices', '耳机音效'],
] as const;
const accessLabels: Record<EffectAccess, string> = {
  allowed: '当前账号拥有使用权益',
  trial: '当前账号拥有试用权益',
  unavailable: '当前账号没有使用权益',
  unknown: '暂未确认当前账号的权益',
};
type Category = EffectCatalogueKind | 'devices';
const kind = ref<Category>('highly_recommended');
const items = ref<EffectCatalogueItem[]>([]);
const trialEffects = ref(new Set<number>());
let trialTimer: ReturnType<typeof setTimeout> | undefined;

function clearTrials() {
  clearTimeout(trialTimer);
  trialTimer = undefined;
  trialEffects.value = new Set();
}

function scheduleTrials(rows: EffectCatalogueItem[]) {
  const receivedAt = performance.now();
  const expiry = new Map<number, number>();
  for (const row of rows) {
    const remaining = row.trial_remaining_ms;
    if (row.membership && remaining && Number.isSafeInteger(remaining) && remaining > 0)
      expiry.set(row.id, receivedAt + remaining);
  }
  function update() {
    const now = performance.now();
    let next = Infinity;
    for (const [id, deadline] of expiry) {
      if (deadline <= now) expiry.delete(id);
      else next = Math.min(next, deadline);
    }
    trialEffects.value = new Set(expiry.keys());
    trialTimer = expiry.size
      ? setTimeout(update, Math.min(2_147_483_647, Math.ceil(next - now)))
      : undefined;
  }
  update();
}
const selected = ref<EffectCatalogueItem>();
const access = ref<EffectAccess>();
const browsing = ref(!props.initialManagementOnly);
const browsingHeadphones = ref(false);
const pending = ref<PendingEffectRegistration[]>([]);
const pendingError = ref('');
const pendingLoading = ref(false);
const recoveryOnly = ref(false);
let pendingRequest = 0;
const loading = ref(false);
const checking = ref(false);
const listError = ref('');
const detailError = ref('');
let listRequest = 0;
let detailRequest = 0;
let previousFocus: HTMLElement | null = null;

function artworkFor(item: EffectCatalogueItem) {
  const url = appearance.value === 'dark' ? item.dark_artwork : item.light_artwork;
  return url && !failedArtwork.value.has(url) ? url : undefined;
}

async function load(category: Category) {
  if (props.account.status !== 'authenticated') return;
  const epoch = props.account.epoch;
  const request = ++listRequest;
  const current = () =>
    listRequest === request &&
    props.account.epoch === epoch &&
    props.account.status === 'authenticated';
  detailRequest++;
  kind.value = category;
  items.value = [];
  clearTrials();
  selected.value = undefined;
  recoveryOnly.value = false;
  access.value = undefined;
  checking.value = false;
  detailError.value = '';
  listError.value = '';
  if (category === 'devices') {
    loading.value = false;
    return;
  }
  loading.value = true;
  try {
    const rows = await core.effectCatalogue(epoch, category);
    if (current()) {
      items.value = rows;
      scheduleTrials(rows);
    }
  } catch (issue) {
    if (current()) listError.value = failure(issue).message;
  } finally {
    if (current()) loading.value = false;
  }
}

async function inspect(item: EffectCatalogueItem) {
  const epoch = props.account.epoch;
  const request = ++detailRequest;
  const current = () =>
    detailRequest === request &&
    props.account.epoch === epoch &&
    props.account.status === 'authenticated';
  selected.value = item;
  recoveryOnly.value = false;
  access.value = undefined;
  detailError.value = '';
  checking.value = true;
  try {
    const detail = await core.effectDetail(epoch, item.id);
    if (current()) {
      if (detail?.id === item.id) access.value = detail.access;
      else detailError.value = '此音效详情暂不可用';
    }
  } catch (issue) {
    if (current()) detailError.value = failure(issue).message;
  } finally {
    if (current()) checking.value = false;
  }
}

async function refreshPending() {
  if (props.account.status !== 'authenticated') return;
  const epoch = props.account.epoch;
  const request = ++pendingRequest;
  const current = () =>
    request === pendingRequest &&
    epoch === props.account.epoch &&
    props.account.status === 'authenticated';
  pendingLoading.value = true;
  pendingError.value = '';
  try {
    const rows = await core.effectPeqPending(epoch);
    if (current()) pending.value = rows;
  } catch {
    if (current()) pendingError.value = '未能读取本机音效登记';
  } finally {
    if (current()) pendingLoading.value = false;
  }
}

function browseCatalogue() {
  browsing.value = true;
  void load(kind.value);
}

async function openRegistration(id: number) {
  detailRequest++;
  selected.value = items.value.find((item) => item.id === id) ?? {
    id,
    name: `音效 ${id}`,
    subtitle: '',
  };
  recoveryOnly.value = true;
  checking.value = false;
  access.value = undefined;
  detailError.value = '';
  await nextTick();
  if (selected.value?.id === id && recoveryOnly.value)
    dialog.value?.querySelector<HTMLElement>('.effect-parametric')?.focus({ preventScroll: true });
}

function retryRights() {
  if (!selected.value) return;
  dialog.value
    ?.querySelector<HTMLButtonElement>('.effect-directory-card[aria-pressed="true"]')
    ?.focus({ preventScroll: true });
  void inspect(selected.value);
}

async function browseHeadphoneCatalogue() {
  browsingHeadphones.value = true;
  await nextTick();
  if (dialog.value?.open)
    dialog.value.querySelector<HTMLButtonElement>('header button')?.focus({ preventScroll: true });
}

watch(
  () => [props.account.epoch, props.account.status],
  () => emit('close'),
);
useEffectRegistrationChanges(
  () => props.account.epoch,
  () => {
    if (!props.headphoneManagement) void refreshPending();
  },
);
onMounted(() => {
  previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  dialog.value?.showModal();
  if (props.headphoneManagement) return;
  if (browsing.value) void load(kind.value);
  void refreshPending();
});
onUnmounted(() => {
  clearTrials();
  listRequest++;
  detailRequest++;
  pendingRequest++;
  previousFocus?.focus({ preventScroll: true });
});
</script>

<template>
  <dialog
    ref="dialog"
    class="effect-directory"
    :aria-label="headphoneManagement ? '耳机音效' : '音效目录'"
    @cancel.prevent="emit('close')"
  >
    <header>
      <h2>{{ headphoneManagement ? '耳机音效' : '音效目录' }}</h2>
      <button
        class="icon-button"
        :aria-label="headphoneManagement ? '关闭耳机音效管理' : '关闭音效目录'"
        @click="emit('close')"
      >
        <Icon name="close" />
      </button>
    </header>
    <div class="effect-directory-body">
      <p v-if="!headphoneManagement" class="effect-directory-note">
        查看音效介绍和账号权益。可验证的独立参数均衡组件可以单独配置。
      </p>
      <div v-if="account.status !== 'authenticated'" class="effect-directory-message">
        <p>登录后可查询音效目录和账号权益。</p>
        <button class="primary" type="button" @click="emit('login')">登录</button>
      </div>
      <template v-else-if="headphoneManagement">
        <EffectDeviceDirectory
          :account="account"
          :applied="deviceParametric"
          :parametric-processing="parametricProcessing"
          :catalogue-parametric="bindingStatus"
          :management-only="!browsingHeadphones"
          @browse="browseHeadphoneCatalogue"
        >
          <template #management-actions>
            <button class="headphone-equalizer-link" type="button" @click="emit('equalizer')">
              打开均衡器
            </button>
          </template>
        </EffectDeviceDirectory>
      </template>
      <template v-else>
        <section
          v-if="pending.length || pendingError || !browsing"
          class="effect-registration"
          aria-label="本机音效登记"
        >
          <p v-if="pendingError" role="status">{{ pendingError }}</p>
          <p v-else-if="!browsing && !pending.length" role="status">
            {{ pendingLoading ? '正在读取本机音效登记…' : '没有待处理的本机音效登记。' }}
          </p>
          <button
            v-if="pendingError"
            type="button"
            :disabled="pendingLoading"
            @click="refreshPending"
          >
            重新读取登记
          </button>
          <template v-if="pending.length">
            <p>本机音效登记</p>
            <button
              v-for="registration in pending"
              :key="registration.id"
              type="button"
              :data-effect-registration="registration.id"
              @click="openRegistration(registration.id)"
            >
              管理
              {{
                items.find((item) => item.id === registration.id)?.name || `音效 ${registration.id}`
              }}
              <span
                v-if="registration.phase === 'stop_uncertain' || registration.phase === 'releasing'"
              >
                · 停止待核对
              </span>
            </button>
          </template>
        </section>
        <button v-if="!browsing" type="button" @click="browseCatalogue">浏览音效目录</button>
        <template v-else>
          <nav aria-label="音效分类">
            <button
              v-for="[key, label] in categories"
              :key="key"
              type="button"
              :aria-pressed="kind === key"
              @click="kind !== key && load(key)"
            >
              {{ label }}
            </button>
          </nav>
          <EffectDeviceDirectory
            v-if="kind === 'devices'"
            :account="account"
            :applied="deviceParametric"
            :parametric-processing="parametricProcessing"
            :catalogue-parametric="bindingStatus"
          />
          <div v-else-if="loading" role="status" class="effect-directory-message">
            正在读取音效目录…
          </div>
          <div v-else-if="listError" class="effect-directory-message">
            <p role="alert">{{ listError }}</p>
            <button type="button" @click="load(kind)">重试目录</button>
          </div>
          <p v-else-if="!items.length" class="effect-directory-message">此分类暂无音效。</p>
          <ul v-else class="effect-directory-grid">
            <li v-for="item in items" :key="item.id">
              <button
                class="effect-directory-card"
                :class="{ 'with-artwork': artworkFor(item) }"
                :aria-pressed="selected?.id === item.id"
                type="button"
                @click="inspect(item)"
              >
                <img
                  v-if="artworkFor(item)"
                  :key="artworkFor(item)"
                  :src="artworkFor(item)"
                  loading="lazy"
                  referrerpolicy="no-referrer"
                  alt=""
                  @error="failedArtwork.add(($event.currentTarget as HTMLImageElement).src)"
                />
                <Icon v-else name="music" />
                <strong>
                  {{ item.name || '未命名音效' }}
                  <span
                    v-if="item.membership"
                    class="effect-card-membership"
                    :data-effect-membership="item.membership"
                    :data-effect-limited-free="trialEffects.has(item.id) || undefined"
                    >{{ item.membership === 'svip' ? 'SVIP' : 'VIP'
                    }}{{ trialEffects.has(item.id) ? ' 限免' : '' }}</span
                  >
                </strong>
                <span v-if="item.subtitle">{{ item.subtitle }}</span>
              </button>
            </li>
          </ul>
        </template>
      </template>
    </div>
    <section
      v-if="selected && account.status === 'authenticated'"
      class="effect-directory-detail"
      aria-label="音效权益"
    >
      <h3>{{ selected.name || '未命名音效' }}</h3>
      <p v-if="recoveryOnly">正在管理本机保留的登记。</p>
      <p v-else-if="checking" role="status">正在查询账号权益…</p>
      <template v-else-if="detailError">
        <p role="alert">{{ detailError }}</p>
        <button type="button" @click="retryRights">重试权益</button>
      </template>
      <p v-else-if="access" role="status">{{ accessLabels[access] }}</p>
      <button v-if="access === 'unknown' && !checking" type="button" @click="retryRights">
        重新查询权益
      </button>
      <EffectParametricControls
        :key="`${account.epoch}:${selected.id}:${recoveryOnly}`"
        :epoch="account.epoch"
        :effect-id="selected.id"
        :allowed="!recoveryOnly && access === 'allowed'"
        :binding-status="bindingStatus"
        :parametric-processing="parametricProcessing"
      />
    </section>
  </dialog>
</template>

<style scoped>
.effect-directory {
  width: min(590px, calc(100vw - 32px));
  max-height: calc(100dvh - 32px);
  padding: 22px 28px;
  overflow: hidden;
  color: var(--text);
  background: var(--surface-raised);
  border: 1px solid var(--line);
  border-radius: 14px;
}
.effect-directory[open] {
  display: flex;
  flex-direction: column;
}
.effect-directory::backdrop {
  background: #0004;
}
.effect-directory > header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex: none;
}
.effect-directory h2 {
  margin: 0;
  font-size: 20px;
}
.effect-directory-body {
  min-height: 0;
  overflow-y: auto;
}
.effect-directory-note,
.effect-directory-message,
.effect-directory-card span {
  color: var(--text-secondary);
}
.headphone-equalizer-link {
  min-height: 34px;
  padding: 4px 12px;
  border: 1px solid var(--line);
  border-radius: 999px;
}
.headphone-equalizer-link:focus-visible,
.headphone-equalizer-link:hover {
  border-color: var(--accent);
}
.effect-directory nav {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-block: 12px;
}
.effect-directory nav button[aria-pressed='true'] {
  color: var(--accent);
  border-color: var(--accent);
}
.effect-registration {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  padding: 8px;
  background: var(--surface-alt);
  border-radius: 8px;
}
.effect-registration p {
  margin: 0 4px 0 0;
}
.effect-directory-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(124px, 1fr));
  gap: 10px;
  padding: 0;
  list-style: none;
}
.effect-directory-card {
  position: relative;
  isolation: isolate;
  display: flex;
  width: 100%;
  min-height: 110px;
  padding: 10px;
  flex-direction: column;
  align-items: start;
  justify-content: end;
  gap: 4px;
  text-align: start;
  overflow-wrap: anywhere;
  background: var(--surface-alt);
  border: 1px solid var(--line);
  border-radius: 8px;
}
.effect-directory-card img,
.effect-directory-card.with-artwork::before {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  pointer-events: none;
}
.effect-directory-card img {
  z-index: -2;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.effect-directory-card.with-artwork::before {
  content: '';
  z-index: -1;
  background: linear-gradient(transparent, #000b);
}
.effect-directory-card.with-artwork,
.effect-directory-card.with-artwork span {
  color: #fff;
}
.effect-card-membership {
  display: inline-block;
  margin-inline-start: 4px;
  padding-inline: 4px;
  border: 1px solid currentColor;
  border-radius: 4px;
  font-size: 10px;
  line-height: 14px;
  white-space: nowrap;
}
.effect-directory-card[aria-pressed='true'] {
  border-color: var(--accent);
}
.effect-directory-card:focus-visible {
  outline-offset: -3px;
}
.effect-directory-detail {
  flex: none;
  max-height: min(210px, 40dvh);
  overflow-y: auto;
  border-top: 1px solid var(--line);
  margin-top: 14px;
  padding-block: 10px;
}
.effect-directory-detail h3,
.effect-directory-detail p {
  margin: 0 0 6px;
  overflow-wrap: anywhere;
}
</style>
