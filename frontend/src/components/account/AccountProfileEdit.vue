<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import { useAccountProfileEdit } from '../../composables/useAccountProfileEdit';
import {
  accountDate,
  accountGender,
  accountRegion,
  accountRegions,
} from '../../services/accountDetail';
import { profileBirthdayToday, type ProfileEditSnapshot } from '../../services/accountProfileEdit';
import { useCore } from '../../services/core';
import { nicknameLength } from '../../services/nickname';
import Icon from '../Icon.vue';
import '../../styles/account-profile-edit.css';

const props = defineProps<{ epoch: string; active: boolean; avatarBusy?: boolean }>();
const emit = defineEmits<{
  close: [];
  saved: [snapshot: ProfileEditSnapshot, confirmed: boolean];
  expired: [];
}>();
defineSlots<{ avatar?: () => unknown }>();
const root = ref<HTMLElement>();
const heading = ref<HTMLElement>();
const recoveryHeading = ref<HTMLElement>();
const dialog = ref<HTMLDialogElement>();
const prompt = ref(false);
let afterDiscard: (() => void) | undefined;
let afterSave: (() => void) | undefined;
let dialogReturn: HTMLElement | undefined;

const {
  draft,
  blocked,
  unseen,
  busy,
  sending,
  loading,
  error,
  notice,
  changes,
  canSave,
  refreshRequired,
  nicknameTaken,
  read,
  save,
  reconcile,
  acceptCurrent,
  refreshForAvatar,
} = useAccountProfileEdit(
  useCore(),
  () => (props.active ? props.epoch : null),
  (snapshot, confirmed) => {
    const resume = afterSave;
    afterSave = undefined;
    emit('saved', snapshot, confirmed);
    if (confirmed) resume?.();
  },
  () => emit('expired'),
);
const recovery = computed(() => !!blocked.value || (!!unseen.value && !sending.value));
const state = computed(() =>
  busy.value
    ? 'saving'
    : recovery.value
      ? 'recovery'
      : loading.value
        ? 'loading'
        : draft.value
          ? 'ready'
          : 'error',
);
const today = profileBirthdayToday();
const yearMax = Number(today.slice(0, 4));
const years = Array.from({ length: yearMax - 1919 }, (_, index) => yearMax - index);
const date = computed(() => (draft.value?.birthday ?? '').split('-').map(Number));
const days = computed(() => new Date(date.value[0] ?? 0, date.value[1] ?? 0, 0).getDate());
const cities = computed(() =>
  Object.entries(accountRegions.find(({ id }) => id === draft.value?.province)?.cities ?? {}),
);
const observed = computed(() => blocked.value?.observed?.profile);
const recoveryFields = computed(() => {
  const intended = blocked.value?.submitted;
  const current = observed.value;
  if (!intended) return [];
  const { nickname, signature, gender, birthday, region } = intended;
  return [
    { key: 'nickname', label: '昵称', intended: nickname, current: current?.nickname },
    { key: 'signature', label: '简介', intended: signature, current: current?.signature },
    {
      key: 'gender',
      label: '性别',
      intended: gender && accountGender(gender),
      current: accountGender(current?.gender),
    },
    {
      key: 'birthday',
      label: '生日',
      intended: birthday && [birthday.year, birthday.month, birthday.day].join('.'),
      current: accountDate(current?.birthday),
    },
    {
      key: 'region',
      label: '地区',
      intended: region && accountRegion(region.province, region.city),
      current: accountRegion(current?.province, current?.city),
    },
  ].filter(({ intended }) => intended !== undefined);
});
function setDate(part: 0 | 1 | 2, event: Event) {
  if (!draft.value) return;
  const value = Number((event.target as HTMLSelectElement).value);
  const [year = 0, month = 0] = date.value;
  if (!value) return;
  const newDate = part === 0 ? [value, 1, 1] : part === 1 ? [year, value, 1] : [year, month, value];
  draft.value.birthday = newDate.map((item) => String(item).padStart(2, '0')).join('-');
}
function setProvince(event: Event) {
  if (!draft.value) return;
  const province = Number((event.target as HTMLSelectElement).value);
  const region = accountRegions.find(({ id }) => id === province);
  if (!region) return;
  draft.value.province = region.id;
  draft.value.city = Number(Object.keys(region.cities)[0]) || '';
}
async function requestLeave(commit: () => void) {
  if (busy.value || prompt.value || props.avatarBusy) return;
  if (!changes.value?.dirty || recovery.value) {
    commit();
    return;
  }
  const active = document.activeElement;
  dialogReturn = active instanceof HTMLElement ? active : undefined;
  afterDiscard = commit;
  prompt.value = true;
  await nextTick();
  dialog.value?.showModal();
}
function dismissPrompt(restore = true) {
  dialog.value?.close();
  prompt.value = false;
  const origin = dialogReturn;
  dialogReturn = undefined;
  if (restore && origin?.isConnected) origin.focus({ preventScroll: true });
}
function keepEditing() {
  afterDiscard = undefined;
  dismissPrompt();
}
function discard() {
  const resume = afterDiscard;
  afterDiscard = undefined;
  dismissPrompt(false);
  resume?.();
}
async function saveDraft(leave = false) {
  if (!canSave.value || props.avatarBusy) return;
  if (leave) {
    afterSave = afterDiscard;
    afterDiscard = undefined;
    dismissPrompt(false);
  }
  const active = document.activeElement;
  await save();
  afterSave = undefined;
  if (!root.value?.isConnected || !props.active) return;
  if (active === document.activeElement || document.activeElement === document.body)
    (recovery.value ? recoveryHeading.value : heading.value)?.focus({ preventScroll: true });
}
async function recover(action: () => unknown) {
  const active = document.activeElement;
  await action();
  if (!root.value?.isConnected || !props.active) return;
  if (active === document.activeElement || document.activeElement === document.body)
    (recovery.value ? recoveryHeading.value : heading.value)?.focus({ preventScroll: true });
}
onMounted(() => {
  heading.value?.focus({ preventScroll: true });
  void read();
});
onBeforeUnmount(() => {
  afterDiscard = undefined;
  afterSave = undefined;
  dialog.value?.close();
  if (root.value) {
    root.value.inert = true;
    root.value.hidden = true;
  }
});
defineExpose({
  requestLeave,
  refreshForAvatar,
  avatarAvailable: computed(() => !busy.value && !loading.value),
});
</script>

<template>
  <section
    ref="root"
    class="account-read-page account-profile-edit-page"
    :data-profile-edit-state="state"
    :aria-busy="loading || busy"
    aria-labelledby="profile-edit-title"
  >
    <header class="account-read-heading">
      <button
        type="button"
        class="icon-button"
        aria-label="返回个人资料"
        :disabled="busy"
        @click="requestLeave(() => emit('close'))"
      >
        <Icon name="back" />
      </button>
      <div>
        <h1 id="profile-edit-title" ref="heading" tabindex="-1">编辑个人信息</h1>
      </div>
    </header>
    <div class="account-read-scroll" tabindex="0" aria-label="编辑个人信息">
      <p v-if="loading && !draft && !recovery" class="account-read-status" role="status">
        <span class="spinner" aria-hidden="true" />正在读取个人资料…
      </p>
      <p v-if="error" class="profile-edit-message profile-edit-error" role="alert">{{ error }}</p>
      <p v-if="notice" class="profile-edit-message" role="status">{{ notice }}</p>
      <section
        v-if="recovery"
        class="profile-edit-recovery"
        aria-labelledby="profile-edit-recovery-title"
      >
        <h2 id="profile-edit-recovery-title" ref="recoveryHeading" tabindex="-1">核对保存结果</h2>
        <p v-if="unseen">尚未取得保存回执。可以重新核对，确认前不能再次保存。</p>
        <p v-else>保存结果尚未确认。请核对当前资料，避免重复提交。</p>
        <dl v-if="recoveryFields.length" class="account-profile-fields" aria-label="待核对的修改">
          <div
            v-for="field in recoveryFields"
            :key="field.key"
            :data-profile-recovery-field="field.key"
          >
            <dt>{{ field.label }}</dt>
            <dd>
              <div>拟修改：{{ field.intended || '未填写' }}</div>
              <div v-if="observed">当前资料：{{ field.current || '未提供' }}</div>
            </dd>
          </div>
        </dl>
        <div class="profile-edit-actions">
          <button
            type="button"
            class="primary"
            data-testid="profile-edit-reconcile"
            :disabled="busy || loading"
            @click="recover(reconcile)"
          >
            {{ busy || loading ? '正在核对…' : '核对保存结果' }}
          </button>
          <button
            v-if="observed"
            type="button"
            class="secondary"
            data-testid="profile-edit-accept-current"
            :disabled="busy || loading"
            @click="recover(acceptCurrent)"
          >
            采用当前资料
          </button>
        </div>
      </section>
      <button
        v-if="(!draft && !loading && !recovery) || refreshRequired"
        type="button"
        class="secondary"
        data-testid="profile-edit-retry"
        :disabled="busy || loading"
        @click="recover(read)"
      >
        重新读取
      </button>
      <div v-if="draft && !recovery" class="profile-edit-content">
        <form class="profile-edit-form" @submit.prevent="saveDraft()">
          <fieldset :disabled="busy || loading" class="profile-edit-fields">
            <div class="profile-edit-field">
              <label for="profile-edit-nickname">昵称：</label>
              <div>
                <input
                  id="profile-edit-nickname"
                  v-model="draft.nickname"
                  data-testid="profile-edit-nickname"
                  :aria-invalid="!!changes?.issues.nickname || nicknameTaken"
                  aria-describedby="profile-edit-nickname-error"
                  maxlength="120"
                  autocomplete="off"
                />
                <div id="profile-edit-nickname-error" class="profile-edit-field-error">
                  {{ changes?.issues.nickname || (nicknameTaken ? error : '') }}
                </div>
              </div>
            </div>
            <div class="profile-edit-field">
              <label for="profile-edit-signature">简介：</label>
              <div class="profile-edit-signature">
                <textarea
                  id="profile-edit-signature"
                  v-model="draft.signature"
                  data-testid="profile-edit-signature"
                  :aria-invalid="!!changes?.issues.signature"
                  aria-describedby="profile-edit-signature-error"
                  maxlength="300"
                  rows="4"
                />
                <div class="profile-edit-signature-meta">
                  <span id="profile-edit-signature-error" class="profile-edit-field-error">{{
                    changes?.issues.signature
                  }}</span>
                  <span>{{ nicknameLength(draft.signature) }}/300</span>
                </div>
              </div>
            </div>
            <div class="profile-edit-field">
              <span id="profile-edit-gender">性别：</span>
              <div class="profile-edit-inline" role="group" aria-labelledby="profile-edit-gender">
                <label
                  ><input
                    v-model="draft.gender"
                    type="radio"
                    name="profile-gender"
                    :value="1"
                    data-testid="profile-edit-gender-male"
                  />男</label
                >
                <label
                  ><input
                    v-model="draft.gender"
                    type="radio"
                    name="profile-gender"
                    :value="2"
                    data-testid="profile-edit-gender-female"
                  />女</label
                >
              </div>
            </div>
            <div class="profile-edit-field">
              <span id="profile-edit-birthday">生日：</span>
              <div>
                <div
                  class="profile-edit-selects"
                  role="group"
                  aria-labelledby="profile-edit-birthday"
                >
                  <select
                    aria-label="出生年份"
                    data-testid="profile-edit-year"
                    :value="date[0] || ''"
                    @change="setDate(0, $event)"
                  >
                    <option v-if="!date[0]" value="" disabled>年</option>
                    <option v-else-if="!years.includes(date[0])" :value="date[0]" disabled>
                      {{ date[0] }} 年
                    </option>
                    <option v-for="year in years" :key="year" :value="year">{{ year }} 年</option>
                  </select>
                  <select
                    aria-label="出生月份"
                    data-testid="profile-edit-month"
                    :value="date[1] || ''"
                    :disabled="!date[0]"
                    @change="setDate(1, $event)"
                  >
                    <option v-if="!date[1]" value="" disabled>月</option>
                    <option v-for="month in 12" :key="month" :value="month">{{ month }} 月</option>
                  </select>
                  <select
                    aria-label="出生日期"
                    data-testid="profile-edit-day"
                    :value="date[2] || ''"
                    :disabled="!date[1]"
                    @change="setDate(2, $event)"
                  >
                    <option v-if="!date[2]" value="" disabled>日</option>
                    <option v-for="day in days || 0" :key="day" :value="day">{{ day }} 日</option>
                  </select>
                </div>
                <div v-if="changes?.issues.birthday" class="profile-edit-field-error">
                  {{ changes.issues.birthday }}
                </div>
              </div>
            </div>
            <div class="profile-edit-field">
              <span id="profile-edit-region">地区：</span>
              <div>
                <div
                  class="profile-edit-selects"
                  role="group"
                  aria-labelledby="profile-edit-region"
                >
                  <select
                    aria-label="省份或地区"
                    data-testid="profile-edit-province"
                    :value="draft.province"
                    @change="setProvince"
                  >
                    <option v-if="!draft.province" value="" disabled>选择省份或地区</option>
                    <option
                      v-else-if="!accountRegions.some(({ id }) => id === draft?.province)"
                      :value="draft.province"
                      disabled
                    >
                      省份代码 {{ draft.province }}
                    </option>
                    <option v-for="region in accountRegions" :key="region.id" :value="region.id">
                      {{ region.name }}
                    </option>
                  </select>
                  <select
                    v-model.number="draft.city"
                    aria-label="城市"
                    data-testid="profile-edit-city"
                    :disabled="!cities.length"
                  >
                    <option v-if="!draft.city" value="" disabled>选择城市</option>
                    <option
                      v-else-if="!cities.some(([id]) => Number(id) === draft?.city)"
                      :value="draft.city"
                      disabled
                    >
                      城市代码 {{ draft.city }}
                    </option>
                    <option v-for="[id, label] in cities" :key="id" :value="Number(id)">
                      {{ label }}
                    </option>
                  </select>
                </div>
                <div v-if="changes?.issues.region" class="profile-edit-field-error">
                  {{ changes.issues.region }}
                </div>
              </div>
            </div>
          </fieldset>
          <div class="profile-edit-actions">
            <button
              type="submit"
              class="primary"
              data-testid="profile-edit-save"
              :disabled="!canSave || avatarBusy"
            >
              {{ busy ? '正在保存…' : '保存' }}
            </button>
            <button
              type="button"
              class="secondary"
              data-testid="profile-edit-cancel"
              :disabled="busy"
              @click="requestLeave(() => emit('close'))"
            >
              取消
            </button>
          </div>
        </form>
        <slot name="avatar" />
      </div>
    </div>
    <dialog
      v-if="prompt"
      ref="dialog"
      class="playlist-dialog profile-edit-leave-dialog"
      aria-labelledby="profile-edit-leave-title"
      @cancel.prevent="keepEditing"
    >
      <header>
        <h2 id="profile-edit-leave-title">修改未保存</h2>
        <button type="button" class="icon-button" aria-label="关闭未保存提示" @click="keepEditing">
          <Icon name="close" />
        </button>
      </header>
      <p>修改的信息尚未保存，是否保存后离开？</p>
      <footer>
        <button type="button" class="secondary" autofocus @click="keepEditing">继续编辑</button>
        <button type="button" class="secondary" data-testid="profile-edit-discard" @click="discard">
          放弃修改
        </button>
        <button
          type="button"
          class="primary"
          data-testid="profile-edit-save-and-leave"
          :disabled="!canSave || avatarBusy"
          @click="saveDraft(true)"
        >
          保存
        </button>
      </footer>
    </dialog>
  </section>
</template>
