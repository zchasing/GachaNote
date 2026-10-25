<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { commands, type Pong } from './bindings'

// M0 空壳：启动时调用后端 ping，验证前后端类型绑定跑通
const pong = ref<Pong | null>(null)
const error = ref<string | null>(null)

onMounted(async () => {
  try {
    pong.value = await commands.ping()
  } catch (e) {
    error.value = String(e)
  }
})
</script>

<template>
  <main class="shell">
    <h1>GachaNote</h1>
    <p v-if="pong" data-test="status">后端已连接 · v{{ pong.appVersion }}</p>
    <p v-else-if="error" data-test="status" class="error">后端连接失败：{{ error }}</p>
    <p v-else data-test="status">正在连接后端…</p>
  </main>
</template>

<style scoped>
.shell {
  display: grid;
  place-content: center;
  min-height: 100vh;
  text-align: center;
}

h1 {
  margin: 0;
  color: var(--accent);
}

p {
  color: var(--muted);
}

.error {
  color: #d9534f;
}
</style>
