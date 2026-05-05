<script setup lang="ts">
defineProps<{
  /** Accent tone for the panel border and icon highlights. */
  tone?: string
  /** When true, allows the panel to lift slightly on hover. */
  interactive?: boolean
}>()
</script>

<template>
  <div
    class="glass-panel"
    :class="{ 'glass-panel--interactive': interactive }"
    :style="tone ? { '--tone': tone } : undefined"
  >
    <div class="glass-panel__content">
      <slot />
    </div>
  </div>
</template>

<style lang="scss" scoped>
@use '../styles/variables' as *;

.glass-panel {
  @include glass-card;
  border-radius: $radius-2xl;
  isolation: isolate;

  &::before {
    content: '';
    position: absolute;
    inset: 0;
    pointer-events: none;
    border-radius: inherit;
    border: 1px solid color-mix(in oklab, var(--tone, var(--accent-strong)) 12%, transparent);
    opacity: 0.8;
  }

  &--interactive {
    cursor: pointer;
    &:hover {
      transform: translateY(-3px);
      border-color: $border-hover;
      box-shadow:
        inset 0 1px 0 rgba(255, 255, 255, 0.06),
        $shadow-strong,
        0 0 0 1px color-mix(in oklab, var(--tone, var(--accent-strong)) 10%, transparent);
    }
  }
}

.glass-panel__content {
  position: relative;
  z-index: 1;
}
</style>
