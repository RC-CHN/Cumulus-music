<script setup lang="ts">
import DailyNotificationSettings from './settings/DailyNotificationSettings.vue';
import { computed, nextTick, ref, watch } from 'vue';
import type { AccountView } from '../services/contracts';
import type { SettingsModel } from '../composables/useSettings';
import { useAppearance } from '../composables/useAppearance';
import { useMotion } from '../composables/useMotion';
import { useControlFocus } from '../composables/useControlFocus';
import type { BuiltinAppearance } from '../services/appearance';
import {
  settingLabels,
  settingValueLabel,
  settingsPending,
  settingsOutcome,
  type SettingKey,
  type SettingChange,
  type SettingsOperation,
} from '../services/settings';
import SettingsConfirm from './settings/SettingsConfirm.vue';
import AboutSupport from './settings/AboutSupport.vue';
import ModalTransition from './ModalTransition.vue';
import Icon from './Icon.vue';
import RelaySettings from './settings/RelaySettings.vue';
import QueueClickPreferences from './settings/QueueClickPreferences.vue';
import RecentHistoryPreference from './settings/RecentHistoryPreference.vue';
import PlaybackStartup from './settings/PlaybackStartup.vue';
import PlaybackCache from './settings/PlaybackCache.vue';
import AudioOutput from './settings/AudioOutput.vue';
import LoudnessBalance from './settings/LoudnessBalance.vue';
import InterfaceFont from './settings/InterfaceFont.vue';
import type { PlayerModel } from '../composables/usePlayer';
import WindowBehavior from './settings/WindowBehavior.vue';
import DesktopLyrics from './settings/DesktopLyrics.vue';
import SystemMedia from './settings/SystemMedia.vue';
import Shortcuts from './settings/Shortcuts.vue';
import type { AccountDetailKind } from '../services/accountDetail';
import AccountBindingsPage from './account/AccountBindingsPage.vue';
import AccountBindingsEntry from './account/AccountBindingsEntry.vue';
import AccountProfilePage from './account/AccountProfilePage.vue';
import QuickLoginOffer from './account/QuickLoginOffer.vue';
import LoginDevicesPage from './account/LoginDevicesPage.vue';
import ProfileBlacklistPage from './profile/ProfileBlacklistPage.vue';
import type { ProfileBlacklistResult } from '../services/profileBlacklist';
import type { RelayModel } from '../composables/useRelay';
import type { RelayPublicationModel } from '../composables/useRelayPublication';
import '../styles/settings.css';
const props = defineProps<{
  model: SettingsModel;
  account: AccountView;
  player: PlayerModel;
  relay: RelayModel;
  publication: RelayPublicationModel;
  accountDetail?: AccountDetailKind;
  quickLoginGuide?: boolean;
}>();
const emit = defineEmits<{
  login: [];
  accounts: [];
  quickLoginEnabled: [];
  downloads: [];
  accountDetail: [kind: AccountDetailKind];
  blacklistProfile: [id: string];
  blacklistUpdated: [id: string, result: ProfileBlacklistResult];
  back: [];
  expired: [];
}>();
const settingsScroll = ref<HTMLElement>();
const profilePage = ref<InstanceType<typeof AccountProfilePage>>();
function requestLeave(commit: () => void) {
  if (profilePage.value) profilePage.value.requestLeave(commit);
  else commit();
}
defineExpose({ requestLeave });
let detailOrigin: HTMLElement | undefined;
let detailScroll = 0;
function openDetail(kind: AccountDetailKind, event: MouseEvent) {
  if (props.account.status !== 'authenticated') return;
  detailOrigin = event.currentTarget as HTMLElement;
  detailScroll = settingsScroll.value?.scrollTop ?? 0;
  emit('accountDetail', kind);
}
watch(
  () => props.accountDetail,
  async (kind, previous) => {
    if (kind || !previous) return;
    await nextTick();
    const scroller = settingsScroll.value;
    if (props.accountDetail || !scroller) return;
    if (previous === 'profile_edit' && detailOrigin && scroller.contains(detailOrigin)) {
      // Native dialog close can put focus on another connected control. An
      // inline editor return belongs to the button that opened that editor.
      detailOrigin.focus({ preventScroll: true });
      scroller.scrollTop = detailScroll;
      return;
    }
    scroller.scrollTop = detailScroll;
    const active = document.activeElement;
    if (active === document.body || !active?.isConnected || !active.getClientRects().length) {
      const fallback =
        previous === 'blacklist'
          ? scroller.querySelector<HTMLElement>('[data-open-user-blacklist]')
          : undefined;
      const origin = detailOrigin ?? fallback;
      if (!detailOrigin && fallback) fallback.scrollIntoView({ block: 'nearest' });
      origin?.focus({ preventScroll: true });
    }
  },
);
const appearance = useAppearance();
const themes: { value: BuiltinAppearance; label: string; icon: string }[] = [
  { value: 'system', label: '跟随系统', icon: 'monitor' },
  { value: 'light', label: '浅色', icon: 'sun' },
  { value: 'dark', label: '深色', icon: 'moon' },
];
const groups: {
  key: SettingKey;
  description: string;
  options: { value: number; label: string }[];
}[] = [
  {
    key: 'shareSetting',
    description: '控制接收私信的范围',
    options: [
      { value: 0, label: '所有人' },
      { value: 1, label: '我关注的人' },
    ],
  },
  {
    key: 'playRecordSetting',
    description: '此项可能联动更新听歌记录与音乐社交的可见范围',
    options: [
      { value: 0, label: '所有人可见' },
      { value: 1, label: '我关注的人可见' },
      { value: 2, label: '仅自己可见' },
    ],
  },
  {
    key: 'personalServiceSetting',
    description: '关闭后，不使用你的个性信息提供个性化服务',
    options: [
      { value: 0, label: '开启' },
      { value: 2, label: '关闭' },
    ],
  },
];
const notices: SettingKey[] = [
  'allowSubscriptionNotify',
  'allowLikedNotify',
  'allowNewFollowerNotify',
];
const field = (key: SettingKey) => props.model.page.value?.fields.find((f) => f.key === key);
const disabled = (key: SettingKey) =>
  !props.model.canChange.value || field(key)?.state !== 'available';
const unavailable = (key: SettingKey) =>
  field(key)?.state === 'unsupported'
    ? '服务端返回了暂不支持的值，已暂停修改'
    : '服务端未返回此设置，已暂停修改';
const operations = computed(() => props.model.page.value?.operations.filter(settingsPending) ?? []);
const latest = computed(() =>
  props.model.page.value?.operations.find((op) => !settingsPending(op)),
);
const lastRead = computed(() =>
  props.model.page.value?.fetchedAt
    ? new Date(props.model.page.value.fetchedAt).toLocaleString('zh-CN', { hour12: false })
    : null,
);
type Confirmation =
  | { kind: 'personal'; change: SettingChange }
  | { kind: 'accept'; op: SettingsOperation; revision: string };
const confirmation = ref<Confirmation>();
const privacyRefresh = ref<HTMLButtonElement>();
const motion = useMotion();
const fallbackFocus = () => privacyRefresh.value ?? null;
const controlFocus = useControlFocus(() => props.account.epoch, fallbackFocus);
watch(
  () => props.account.epoch,
  () => (confirmation.value = undefined),
);
async function choose(change: SettingChange, event: Event) {
  if (disabled(change.key) || field(change.key)?.value === change.value) return;
  if (change.key === 'personalServiceSetting') confirmation.value = { kind: 'personal', change };
  else await controlFocus.run(event, () => props.model.change(change));
}
async function confirm() {
  const request = confirmation.value;
  if (!request) return;
  const ok =
    request.kind === 'personal'
      ? await props.model.change(request.change)
      : await props.model.command({
          kind: 'accept_current',
          id: request.op.id,
          expected_revision: request.revision,
        });
  if (ok || props.model.pending.value || props.model.error.value) confirmation.value = undefined;
}
function accept(op: SettingsOperation) {
  if (props.model.page.value?.revision)
    confirmation.value = { kind: 'accept', op, revision: props.model.page.value.revision };
}
</script>
<template>
  <AccountProfilePage
    v-if="accountDetail === 'profile' || accountDetail === 'profile_edit'"
    :key="accountDetail"
    ref="profilePage"
    :account="account"
    :direct-edit="accountDetail === 'profile_edit'"
    @back="emit('back')"
    @expired="emit('expired')"
  />
  <LoginDevicesPage
    v-else-if="accountDetail === 'devices'"
    :account="account"
    @back="emit('back')"
    @expired="emit('expired')"
  />
  <AccountBindingsPage
    v-else-if="accountDetail === 'bindings'"
    :account="account"
    @back="emit('back')"
    @expired="emit('expired')"
  />
  <ProfileBlacklistPage
    v-else-if="accountDetail === 'blacklist'"
    :account="account"
    @back="emit('back')"
    @open="(id) => emit('blacklistProfile', id)"
    @updated="(id, result) => emit('blacklistUpdated', id, result)"
    @expired="emit('expired')"
  />
  <section
    v-show="!accountDetail"
    class="settings-page"
    :inert="!!accountDetail"
    aria-labelledby="settings-title"
    :aria-busy="model.loading.value || model.busy.value"
  >
    <header class="settings-heading">
      <div>
        <h1 id="settings-title">设置</h1>
        <p class="muted">让音乐更合你的习惯</p>
      </div>
    </header>
    <div ref="settingsScroll" class="settings-scroll" tabindex="0" aria-label="设置内容">
      <section class="settings-section" aria-labelledby="settings-appearance">
        <h2 id="settings-appearance">外观</h2>
        <div class="settings-section-content">
          <fieldset class="settings-options theme-options">
            <legend>界面主题</legend>
            <label
              v-for="theme in themes"
              :key="theme.value"
              :class="{ chosen: appearance.preference.value === theme.value }"
              ><input
                type="radio"
                name="settings-theme"
                :value="theme.value"
                :checked="appearance.preference.value === theme.value"
                @change="appearance.select(theme.value)"
              /><Icon :name="theme.icon" /><span>{{ theme.label }}</span></label
            >
          </fieldset>
          <p class="settings-note">
            当前为{{
              { light: '浅色', dark: '深色', red: '经典红', custom: '自定义按钮颜色' }[
                appearance.effective.value
              ]
            }}模式。主题保存在这台设备。
          </p>
          <p v-if="!appearance.saved.value" role="alert" class="catalog-error">
            主题已应用，但暂时无法保存到本机。
          </p>
          <label class="settings-checkbox motion-option">
            <input
              type="checkbox"
              name="disable-animation"
              :checked="motion.preference.value === 'disabled'"
              @change="motion.select(($event.target as HTMLInputElement).checked)"
            />
            <span>禁用动画效果</span>
          </label>
          <p class="settings-note" role="status">
            {{
              motion.preference.value === 'disabled'
                ? '已关闭界面动画和歌词平滑滚动。'
                : motion.systemReduced.value
                  ? '系统已开启减少动态效果，界面动画已关闭。'
                  : '界面动画已开启；系统开启减少动态效果时会自动关闭。'
            }}
            此设置保存在这台设备，不影响音乐播放。
          </p>
          <p v-if="!motion.saved.value" role="alert" class="catalog-error">
            动画设置已应用，但暂时无法保存到本机。重新选择可再次保存。
          </p>
        </div>
      </section>
      <section class="settings-section" aria-labelledby="settings-general">
        <h2 id="settings-general">常规</h2>
        <div class="settings-section-content"><InterfaceFont :epoch="account.epoch" /></div>
      </section>
      <PlaybackStartup :account="account" />
      <QueueClickPreferences :account="account" />
      <RecentHistoryPreference :account="account" @expired="emit('expired')" />
      <AudioOutput :account="account" />
      <LoudnessBalance :model="player" :epoch="account.epoch" />
      <WindowBehavior :account="account" />
      <DesktopLyrics :account="account" />
      <SystemMedia :account="account" />
      <Shortcuts :account="account" />
      <section class="settings-section" aria-labelledby="settings-account">
        <h2 id="settings-account">账号</h2>
        <div class="settings-section-content">
          <p>{{ account.status === 'authenticated' ? account.nickname : '尚未登录网易云音乐' }}</p>
          <div class="settings-account-actions">
            <button
              class="secondary"
              @click="account.status === 'authenticated' ? emit('accounts') : emit('login')"
            >
              {{ account.status === 'authenticated' ? '管理登录账号' : '登录账号' }}
            </button>
            <template v-if="account.status === 'authenticated'">
              <AccountBindingsEntry
                :account="account"
                @open="(event) => openDetail('bindings', event)"
                @expired="emit('expired')"
              />
              <button
                type="button"
                class="secondary"
                data-testid="account-profile-edit-direct"
                @click="openDetail('profile_edit', $event)"
              >
                修改个人信息
              </button>
              <button
                type="button"
                class="secondary"
                data-testid="account-profile-open"
                @click="openDetail('profile', $event)"
              >
                查看个人资料
              </button>
              <button
                type="button"
                class="secondary"
                data-testid="account-devices-open"
                @click="openDetail('devices', $event)"
              >
                登录设备管理
              </button>
            </template>
          </div>
          <p
            v-if="quickLoginGuide && account.status === 'authenticated'"
            id="settings-quick-login-guide"
            class="settings-note"
            role="status"
            tabindex="-1"
          >
            使用“切换账号”功能需先开启“快速登录”
          </p>
          <QuickLoginOffer
            v-if="account.status === 'authenticated'"
            :key="account.epoch"
            :epoch="account.epoch"
            @accounts="emit('accounts')"
            @enabled="emit('quickLoginEnabled')"
          />
        </div>
      </section>
      <section class="settings-section" aria-labelledby="settings-privacy">
        <h2 id="settings-privacy" tabindex="-1">消息与隐私</h2>
        <div class="settings-section-content">
          <p v-if="account.status !== 'authenticated'" class="settings-note">
            登录后可查看和修改账号的消息与隐私设置。
          </p>
          <template v-else>
            <div class="settings-read-status">
              <span>我的黑名单</span>
              <button
                class="secondary"
                type="button"
                data-open-user-blacklist
                aria-label="查看我的黑名单"
                @click="openDetail('blacklist', $event)"
              >
                查看
              </button>
            </div>
            <div class="settings-read-status">
              <span>{{
                model.loading.value
                  ? '正在读取账号设置…'
                  : lastRead
                    ? `最近读取：${lastRead}`
                    : '尚未读取账号设置'
              }}</span
              ><button
                ref="privacyRefresh"
                class="text-button"
                aria-label="重新读取账号设置"
                :disabled="model.loading.value || model.busy.value"
                @click="controlFocus.run($event, model.refresh)"
              >
                刷新
              </button>
            </div>
            <p v-if="model.error.value" class="catalog-error" role="alert">
              {{ model.error.value }}
            </p>
            <div v-if="model.unknown.value" class="settings-recovery" role="status">
              <h3>修改的结果尚未返回</h3>
              <p>请先读取处理记录；如果请求还没开始，可以取消。取消不会撤销已经完成的修改。</p>
              <button
                class="secondary"
                :disabled="model.busy.value || model.loading.value"
                @click="controlFocus.run($event, model.localRefresh)"
              >
                读取处理记录</button
              ><button
                class="secondary"
                :disabled="model.busy.value || model.loading.value"
                @click="controlFocus.run($event, model.cancelUnknown)"
              >
                取消未开始的修改
              </button>
            </div>
            <div v-for="op in operations" :key="op.id" class="settings-recovery" role="status">
              <h3>{{ settingLabels[op.change.key] }}：{{ settingsOutcome(op) }}</h3>
              <p>
                目标：{{ settingValueLabel(op.change.key, op.change.value)
                }}<span v-if="op.lastReadAt !== null">
                  · 当前回读：{{ settingValueLabel(op.change.key, op.observed) }}</span
                >
              </p>
              <button
                class="secondary"
                :disabled="model.busy.value || model.loading.value"
                @click="
                  controlFocus.run($event, () => model.command({ kind: 'reconcile', id: op.id }))
                "
              >
                核对服务端状态</button
              ><button
                v-if="op.readRevision && op.readRevision === model.page.value?.revision"
                class="text-button"
                :disabled="model.busy.value || model.loading.value"
                @click="accept(op)"
              >
                保留当前设置
              </button>
            </div>
            <p v-if="model.busy.value" class="settings-note" role="status">正在处理，请稍候…</p>
            <p v-else-if="latest && !model.pending.value" class="settings-outcome" role="status">
              {{ settingLabels[latest.change.key] }}：{{ settingsOutcome(latest)
              }}<span
                v-if="
                  latest.change.key === 'personalServiceSetting' && latest.phase === 'confirmed'
                "
                >。重新打开应用后加载新的推荐内容。</span
              >
            </p>
            <template v-if="model.page.value?.revision">
              <fieldset
                v-for="group in groups"
                :key="group.key"
                class="settings-options privacy-options"
                :disabled="disabled(group.key)"
              >
                <legend>{{ settingLabels[group.key] }}</legend>
                <p class="settings-note">{{ group.description }}</p>
                <div class="settings-choices">
                  <label v-for="option in group.options" :key="option.value"
                    ><input
                      type="radio"
                      :name="group.key"
                      :value="option.value"
                      :checked="field(group.key)?.value === option.value"
                      @click.prevent="choose({ key: group.key, value: option.value }, $event)"
                    /><span>{{ option.label }}</span></label
                  >
                </div>
                <p v-if="field(group.key)?.state !== 'available'" class="settings-note">
                  {{ unavailable(group.key) }}
                </p>
              </fieldset>
              <fieldset class="settings-options privacy-options">
                <legend>消息通知</legend>
                <div class="settings-choices">
                  <label v-for="key in notices" :key="key"
                    ><input
                      type="checkbox"
                      :name="key"
                      :checked="field(key)?.value === true"
                      :disabled="disabled(key)"
                      @click.prevent="choose({ key, value: field(key)?.value !== true }, $event)"
                    /><span>{{ settingLabels[key] }}</span
                    ><small v-if="field(key)?.state !== 'available'"
                      >（{{ field(key)?.state === 'unsupported' ? '值不支持' : '未返回' }}）</small
                    ></label
                  >
                </div>
              </fieldset>
              <details class="settings-linked">
                <summary>查看联动的隐私状态</summary>
                <dl>
                  <dt>允许所有人查看听歌记录</dt>
                  <dd>
                    {{
                      settingValueLabel(
                        'allowPeopleCanSeeMyPlayRecord',
                        model.page.value.linkedPrivacy.peopleCanSeePlayRecord,
                      )
                    }}
                  </dd>
                  <dt>允许关注的人查看听歌记录</dt>
                  <dd>
                    {{
                      settingValueLabel(
                        'allowPeopleCanSeeMyPlayRecord',
                        model.page.value.linkedPrivacy.followedCanSeePlayRecord,
                      )
                    }}
                  </dd>
                </dl>
                <p class="settings-note">展示本次回读的状态。查看权限的实际效果仍由服务端决定。</p>
              </details>
            </template>
          </template>
        </div>
      </section>
      <DailyNotificationSettings :epoch="account.epoch" />
      <RelaySettings
        :epoch="account.epoch"
        :model="relay"
        :publication="publication"
        :signed-in="account.status === 'authenticated'"
      />
      <section class="settings-section" aria-labelledby="settings-download">
        <h2 id="settings-download" tabindex="-1">下载</h2>
        <div class="settings-section-content">
          <p class="settings-note">下载目录、音质、格式和歌词选项可在下载管理中调整。</p>
          <button class="secondary" @click="emit('downloads')">打开下载管理</button>
        </div>
      </section>
      <PlaybackCache />
      <AboutSupport />
    </div>
    <ModalTransition :fallback-focus="fallbackFocus"
      ><SettingsConfirm
        v-if="confirmation"
        :title="
          confirmation.kind === 'accept'
            ? '保留当前设置？'
            : `${settingValueLabel('personalServiceSetting', confirmation.change.value)}个性化服务？`
        "
        :description="
          confirmation.kind === 'accept'
            ? '将以刚读取的服务端设置为准，结束本次核对；不会重发原修改，也不能证明原请求是否生效。'
            : '这会修改账号的个性化服务设置。保存后，重新打开应用加载新的推荐内容。'
        "
        :busy="model.busy.value"
        :error="model.error.value"
        @close="confirmation = undefined"
        @confirm="confirm"
    /></ModalTransition>
  </section>
</template>
