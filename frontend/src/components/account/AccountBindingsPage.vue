<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useAccountRead } from '../../composables/useAccountRead';
import { bindingGroups, bindingMethodName } from '../../services/accountBindings';
import type { AccountView } from '../../services/contracts';
import { useCore } from '../../services/core';
import Icon from '../Icon.vue';
import AccountBindingProviderMark from './AccountBindingProviderMark.vue';
import AccountBindingQrDialog from './AccountBindingQrDialog.vue';
import AccountReadLayout from './AccountReadLayout.vue';

const props = defineProps<{ account: AccountView }>();
const emit = defineEmits<{ back: []; expired: [] }>();
const core = useCore();
const owner = () => (props.account.status === 'authenticated' ? props.account.epoch : null);
const { value, state, error, refresh } = useAccountRead(
  owner,
  (epoch) => core.accountBindings(epoch),
  undefined,
  () => emit('expired'),
);
const groups = computed(() => (value.value ? bindingGroups(value.value) : []));
const qr = ref(false);
watch(owner, () => (qr.value = false), { flush: 'sync' });
</script>

<template>
  <AccountReadLayout
    kind="bindings"
    title="绑定账号"
    subtitle="查看已绑定的登录方式，或用网易云音乐 App 扫码设置。"
    :epoch="account.epoch"
    :state="state"
    :error="error"
    :refresh="refresh"
    @back="emit('back')"
  >
    <p v-if="value && !value.registrationProvider" class="muted binding-missing-registration">
      当前服务未提供注册方式。
    </p>
    <section
      v-for="group in groups"
      :key="group.key"
      :data-binding-group="group.key"
      class="binding-group"
      :aria-labelledby="`binding-group-${group.key}`"
    >
      <h2 :id="`binding-group-${group.key}`">{{ group.title }}</h2>
      <ul>
        <li v-for="row in group.rows" :key="row.method" :data-binding-method="row.method">
          <AccountBindingProviderMark
            class="binding-row-mark"
            :method="row.method"
            :bound="Boolean(row.binding)"
          />
          <div class="binding-description">
            <strong>{{ bindingMethodName[row.method] }}</strong>
            <div v-if="row.detail || row.change || row.binding" class="binding-detail">
              <span v-if="row.detail">{{ row.detail }}</span>
              <span v-else-if="row.binding" class="muted">详情未提供</span>
              <button
                v-if="row.change"
                type="button"
                class="text-button"
                data-binding-action="change"
                @click="qr = true"
              >
                {{ row.change }}
              </button>
            </div>
          </div>
          <div class="binding-actions">
            <button
              v-if="row.password"
              type="button"
              class="secondary"
              data-binding-action="password"
              @click="qr = true"
            >
              {{ row.password }}
            </button>
            <span v-if="row.binding && !row.registered" class="binding-confirmed">
              <Icon name="check" /> 已绑定
            </span>
            <button
              v-else-if="row.mayBind"
              type="button"
              class="secondary"
              data-binding-action="bind"
              :aria-label="`绑定${bindingMethodName[row.method]}`"
              @click="qr = true"
            >
              <Icon name="plus" /> 绑定
            </button>
          </div>
        </li>
      </ul>
    </section>
  </AccountReadLayout>
  <AccountBindingQrDialog v-if="qr && owner()" :epoch="account.epoch" @close="qr = false" />
</template>

<style scoped>
.binding-missing-registration {
  margin: 0 0 14px;
  font-size: 13px;
}
.binding-group {
  max-width: 900px;
  padding: 0 0 20px;
  margin: 0 0 20px;
  border-bottom: 1px solid var(--line);
}
.binding-group h2 {
  margin: 0 0 10px;
  font-size: 16px;
}
.binding-group ul {
  padding: 0;
  margin: 0;
  list-style: none;
}
.binding-group li {
  display: flex;
  align-items: center;
  gap: 16px;
  min-height: 80px;
  padding: 8px clamp(6px, 2vw, 20px);
  border-radius: 14px;
}
.binding-group li:focus-within {
  background: var(--surface-hover);
}
@media (hover: hover) {
  .binding-group li:hover {
    background: var(--surface-hover);
    box-shadow: 0 6px 16px rgb(0 0 0 / 8%);
  }
}
.binding-row-mark {
  --provider-mark-size: 64px;
  --provider-mark-glyph: 36px;
  --provider-mark-label: 15px;
}
.binding-description {
  flex: 1;
  min-width: 0;
  font-size: 14px;
  overflow-wrap: anywhere;
}
.binding-detail {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 3px 10px;
}
.binding-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 5px 8px;
  min-width: 76px;
}
.binding-actions .secondary,
.binding-confirmed {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  min-width: 84px;
  min-height: 28px;
  padding: 2px 8px;
  border-radius: 16px;
  font-size: 13px;
  white-space: nowrap;
}
.binding-confirmed {
  border: 1px solid var(--line);
  color: var(--text-secondary);
}
.binding-actions .icon {
  width: 15px;
  height: 15px;
}
</style>
