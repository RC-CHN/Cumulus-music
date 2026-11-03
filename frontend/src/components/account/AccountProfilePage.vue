<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { AccountView } from '../../services/contracts';
import {
  accountDate,
  accountGender,
  accountRegion,
  bindingLabel,
} from '../../services/accountDetail';
import { useAccountRead } from '../../composables/useAccountRead';
import { useCore } from '../../services/core';
import AccountReadLayout from './AccountReadLayout.vue';
import Icon from '../Icon.vue';
const props = defineProps<{ account: AccountView }>();
const emit = defineEmits<{ back: []; expired: [] }>();
const core = useCore();
const read = useAccountRead(
  () => (props.account.status === 'authenticated' ? props.account.epoch : null),
  (epoch) => core.accountProfileDetail(epoch),
  undefined,
  () => emit('expired'),
);
const { value, state, error, refresh } = read;
const avatarFailed = ref(false);
watch(
  () => value.value?.profile.avatarUrl,
  () => (avatarFailed.value = false),
);
const fields = computed(() => {
  const profile = value.value?.profile;
  if (!profile) return [];
  const region = accountRegion(profile.province, profile.city);
  return [
    { key: 'nickname', label: '昵称', text: profile.nickname },
    { key: 'signature', label: '简介', text: profile.signature },
    { key: 'gender', label: '性别', text: accountGender(profile.gender) },
    { key: 'birthday', label: '生日', text: accountDate(profile.birthday) },
    { key: 'region', label: '地区', text: region },
    { key: 'level', label: '等级', text: profile.level == null ? null : `Lv.${profile.level}` },
    { key: 'uid', label: '用户 ID', text: profile.uid },
  ];
});
</script>
<template>
  <AccountReadLayout
    kind="profile"
    :epoch="account.epoch"
    title="个人资料"
    subtitle="查看当前账号的资料和绑定信息。"
    :state="state"
    :error="error"
    :refresh="refresh"
    @back="emit('back')"
  >
    <template v-if="value">
      <div class="account-profile-content">
        <dl class="account-profile-fields">
          <div v-for="field in fields" :key="field.key" :data-profile-field="field.key">
            <dt>{{ field.label }}</dt>
            <dd>{{ field.text || '未提供' }}</dd>
          </div>
        </dl>
        <div class="account-profile-avatar" role="img" aria-label="当前账号头像">
          <img
            v-if="value.profile.avatarUrl && !avatarFailed"
            :src="value.profile.avatarUrl"
            alt=""
            referrerpolicy="no-referrer"
            @error="avatarFailed = true"
          />
          <Icon v-else name="user" />
        </div>
      </div>
      <section class="account-profile-bindings" aria-labelledby="account-bindings-title">
        <h2 id="account-bindings-title">绑定账号</h2>
        <p v-if="!value.bindings.length" class="account-read-status">暂无绑定记录。</p>
        <ul v-else>
          <li
            v-for="(binding, index) in value.bindings"
            :key="index"
            :data-binding-provider="binding.provider"
          >
            <span class="account-binding-kind">{{ bindingLabel(binding.provider) }}</span>
            <div class="account-binding-detail">
              <strong>已绑定</strong>
              <span v-if="binding.maskedAddress || binding.displayName">
                <template v-if="binding.provider === 'cellphone' && binding.countryCode"
                  >+{{ binding.countryCode }}
                </template>
                {{ binding.maskedAddress || binding.displayName }}
              </span>
              <span v-else-if="!binding.detailsAvailable">详情未提供</span>
              <small v-if="binding.hasPassword != null">{{
                binding.hasPassword ? '已设置密码' : '未设置密码'
              }}</small>
            </div>
          </li>
        </ul>
      </section>
    </template>
  </AccountReadLayout>
</template>
