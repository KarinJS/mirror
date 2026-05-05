<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { ShieldCheck, Zap, GitBranch, Rocket } from 'lucide-vue-next'

const { t } = useI18n()

const features = [
  { key: 'whitelist', icon: ShieldCheck, tone: 'var(--green)' },
  { key: 'instant', icon: Zap, tone: 'var(--orange)' },
  { key: 'routes', icon: GitBranch, tone: 'var(--accent-strong)' },
  { key: 'deploy', icon: Rocket, tone: 'var(--violet)' },
] as const
</script>

<template>
  <section class="features">
    <div class="features-grid">
      <div
        v-for="(feat, idx) in features"
        :key="feat.key"
        class="feature-card"
        :style="{ '--ft': feat.tone }"
        v-motion
        :initial="{ opacity: 0, y: 16 }"
        :visibleOnce="{ opacity: 1, y: 0, transition: { duration: 500, delay: 100 + idx * 80 } }"
      >
        <span class="feature-icon">
          <component :is="feat.icon" :size="18" :stroke-width="1.8" />
        </span>
        <div class="feature-text">
          <span class="feature-title">{{ t(`features.items.${feat.key}.title`) }}</span>
          <span class="feature-desc">{{ t(`features.items.${feat.key}.desc`) }}</span>
        </div>
      </div>
    </div>
  </section>
</template>

<style lang="scss" scoped>
@use '../styles/variables' as *;

.features {
  width: 100%;
  max-width: 1180px;
  margin: 0 auto;
  padding: 0 clamp(16px, 4vw, 36px);
}

.features-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 14px;
}

.feature-card {
  position: relative;
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 18px 16px;
  border: 1px solid $glass-stroke;
  border-radius: $radius-xl;
  background: color-mix(in oklab, $bg-surface 92%, transparent);
  backdrop-filter: blur($glass-blur) saturate(120%);
  overflow: hidden;
  transition: transform $t-base, border-color $t-base, box-shadow $t-base;

  &::before {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    pointer-events: none;
    background: linear-gradient(135deg, color-mix(in oklab, var(--ft) 8%, transparent), transparent 60%);
  }

  &:hover {
    transform: translateY(-2px);
    border-color: $border-hover;
    box-shadow: $shadow-soft;
  }
}

.feature-icon {
  flex-shrink: 0;
  width: 36px;
  height: 36px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: $radius-md;
  background: color-mix(in oklab, var(--ft) 14%, transparent);
  border: 1px solid color-mix(in oklab, var(--ft) 22%, $glass-stroke);
  color: var(--ft);
}

.feature-text {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.feature-title {
  font-size: 13px;
  font-weight: 700;
  color: $text-primary;
  letter-spacing: -0.01em;
}

.feature-desc {
  font-size: 12px;
  line-height: 1.5;
  color: $text-secondary;
}

@media (max-width: 900px) {
  .features-grid { grid-template-columns: repeat(2, 1fr); }
}

@media (max-width: 480px) {
  .features-grid { grid-template-columns: 1fr; }
}
</style>
