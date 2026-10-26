<script setup lang="ts">
// 应用图标：四角星形的浅蓝晶石，金色描边（UI/UX 文档 3.1，自绘原创图形）。
withDefaults(defineProps<{ size?: number }>(), { size: 30 })
</script>

<template>
  <svg :width="size" :height="size" viewBox="0 0 32 32" aria-hidden="true">
    <defs>
      <linearGradient id="gn-gem" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stop-color="#F4FAFF" />
        <stop offset="0.55" stop-color="#B9DBFA" />
        <stop offset="1" stop-color="#6FA8E8" />
      </linearGradient>
    </defs>
    <path
      d="M16 2.5 L20 12 L29.5 16 L20 20 L16 29.5 L12 20 L2.5 16 L12 12 Z"
      fill="url(#gn-gem)"
      stroke="#C8A464"
      stroke-width="1.4"
      stroke-linejoin="round"
    />
    <path d="M16 2.5 L16 29.5 M2.5 16 L29.5 16" stroke="#FFFFFF" stroke-width="0.9" opacity="0.8" />
    <path d="M12 12 L20 20 M20 12 L12 20" stroke="#5B8FD0" stroke-width="0.6" opacity="0.45" />
    <circle cx="16" cy="16" r="1.6" fill="#FFFFFF" />
  </svg>
</template>
