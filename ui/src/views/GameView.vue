<script setup lang="ts">
import { computed, watchEffect } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import GameIcon from '@/components/GameIcon.vue'
import PageHeader from '@/components/PageHeader.vue'
import { useNames } from '@/composables/useNames'
import { useAccountsStore } from '@/stores/accounts'
import { useSettingsStore } from '@/stores/settings'
import { useUiStore } from '@/stores/ui'

// 边栏点击游戏：进入上次查看的账号；没有账号时显示「暂无账号 · 去导入」。
const route = useRoute()
const router = useRouter()
const accounts = useAccountsStore()
const settings = useSettingsStore()
const ui = useUiStore()
const { t, gameName } = useNames()

const game = computed(() => String(route.params.game))
const list = computed(() => accounts.ofGame(game.value))

watchEffect(() => {
  if (list.value.length === 0) return
  const last = settings.settings.lastAccounts[game.value]
  const target = list.value.find((a) => a.id === last) ?? list.value[0]!
  void router.replace(`/account/${target.id}/stats`)
})
</script>

<template>
  <PageHeader :title="gameName(game)" :subtitle="t('nav.noAccount')">
    <template #icon><GameIcon :game="game" :size="32" /></template>
  </PageHeader>
  <div class="content">
    <div v-if="list.length === 0" class="wrap">
      <div class="empty" data-test="empty-game">
        <div>{{ t('account.emptyGame', { game: gameName(game) }) }}</div>
        <button class="btn primary" @click="ui.openImport({ game })">{{ t('account.goImport') }}</button>
      </div>
    </div>
  </div>
</template>
