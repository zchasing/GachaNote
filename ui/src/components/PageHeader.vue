<script setup lang="ts">
// 固定顶栏：上行 图标 + 标题 + 辅助说明 + 操作按钮，下行 40 px 放标签页或筛选（UI/UX 文档第 3 节）。
defineProps<{ title?: string; subtitle?: string }>()
</script>

<template>
  <header class="topbar">
    <div class="inner">
      <div class="thead">
        <div class="row" style="gap: 10px; flex-wrap: nowrap; min-width: 0">
          <slot name="icon" />
          <div style="min-width: 0">
            <slot name="title">
              <h1 class="h1">{{ title }}</h1>
            </slot>
            <div class="muted sub"><slot name="subtitle">{{ subtitle }}</slot></div>
          </div>
        </div>
        <div class="row actions"><slot name="actions" /></div>
      </div>
      <div class="trow"><slot name="tabs" /></div>
    </div>
  </header>
</template>

<style scoped>
.topbar {
  position: sticky;
  top: 0;
  z-index: 10;
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  padding: 16px 32px 0;
  height: 120px;
  flex: 0 0 auto;
}
.inner {
  max-width: 1200px;
  margin: 0 auto;
  height: 100%;
  display: flex;
  flex-direction: column;
}
.thead {
  min-height: 60px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.trow {
  height: 40px;
  margin-top: auto;
  display: flex;
  align-items: flex-end;
}
.h1 {
  font-size: 22px;
  font-weight: 600;
  white-space: nowrap;
}
.sub {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.actions {
  flex-wrap: nowrap;
  gap: 8px;
}
</style>
