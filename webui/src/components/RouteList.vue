<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { Download, FileCode, UserCircle, Package, Globe2, ArrowUpRight } from 'lucide-vue-next'
import GlassPanel from './GlassPanel.vue'

const { t } = useI18n()

interface Route {
  key: 'gh' | 'raw' | 'avatar' | 'unpkg' | 'mirror'
  icon: typeof Download
  tone: string
  path: string
  example: string
}

const routes = computed<Route[]>(() => [
  {
    key: 'gh',
    icon: Download,
    tone: 'var(--accent-strong)',
    path: '/gh/<owner>/<repo>/releases/download/<tag>/<file>',
    example: '/gh/NapNeko/NapCatQQ/releases/download/v4.18.0/NapCat.Framework.zip',
  },
  {
    key: 'raw',
    icon: FileCode,
    tone: 'var(--green)',
    path: '/raw/<owner>/<repo>/<branch>/<path>',
    example: '/raw/karinjs/karin/main/package.json',
  },
  {
    key: 'avatar',
    icon: UserCircle,
    tone: 'var(--orange)',
    path: '/avatar/<user>.png',
    example: '/avatar/karinjs.png',
  },
  {
    key: 'unpkg',
    icon: Package,
    tone: 'var(--rose)',
    path: '/unpkg/<pkg>/<path>',
    example: '/unpkg/karin/package.json',
  },
  {
    key: 'mirror',
    icon: Globe2,
    tone: 'var(--violet)',
    path: '/mirror/<host>/<path>',
    example: '/mirror/example.com/file.zip',
  },
])
</script>

<template>
  <section class="routes" v-motion :initial="{ opacity: 0, y: 24 }" :visibleOnce="{ opacity: 1, y: 0, transition: { duration: 700, delay: 100 } }">
    <header class="routes-head">
      <span class="head-kicker">{{ t('routes.kicker') }}</span>
      <h2 class="head-title">{{ t('routes.title') }}</h2>
      <p class="head-desc">{{ t('routes.desc') }}</p>
    </header>

    <div class="routes-grid">
      <GlassPanel
        v-for="(route, idx) in routes"
        :key="route.key"
        class="route-card"
        :tone="route.tone"
        v-motion
        :initial="{ opacity: 0, y: 18 }"
        :visibleOnce="{ opacity: 1, y: 0, transition: { duration: 600, delay: 180 + idx * 70 } }"
      >
        <div class="route-card__top">
          <span class="icon-shell" :style="{ '--tone': route.tone }">
            <component :is="route.icon" :size="18" :stroke-width="1.8" />
          </span>
          <ArrowUpRight class="corner-arrow" :size="16" :stroke-width="1.8" />
        </div>

        <h3 class="route-name">{{ t(`routes.items.${route.key}.name`) }}</h3>
        <p class="route-desc">{{ t(`routes.items.${route.key}.desc`) }}</p>

        <div class="route-meta">
          <span class="meta-label">{{ t('routes.pathLabel') }}</span>
          <code class="meta-code">{{ route.path }}</code>
        </div>
        <div class="route-meta">
          <span class="meta-label">{{ t('routes.exampleLabel') }}</span>
          <code class="meta-code meta-code--ex">{{ route.example }}</code>
        </div>
      </GlassPanel>
    </div>
  </section>
</template>

<style lang="scss" scoped>
@use '../styles/variables' as *;

.routes {
  width: 100%;
  max-width: 1180px;
  margin: 0 auto;
}

.routes-head {
  text-align: center;
  margin-bottom: 32px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
}

.head-kicker {
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: $accent-strong;
}

.head-title {
  font-family: $font-display;
  font-size: clamp(1.6rem, 3vw, 2.2rem);
  font-weight: 700;
  letter-spacing: -0.025em;
  color: $text-primary;
}

.head-desc {
  max-width: 56ch;
  font-size: 13.5px;
  line-height: 1.65;
  color: $text-secondary;
}

.routes-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 18px;
}

.route-card {
  cursor: default;

  :deep(.glass-panel__content) {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 22px 22px 20px;
  }

  &:hover {
    transform: translateY(-3px);
    .corner-arrow {
      transform: translate(2px, -2px);
      opacity: 1;
    }
    .icon-shell::after { opacity: 0.5; }
  }
}

.route-card__top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 4px;
}

.icon-shell {
  position: relative;
  width: 38px;
  height: 38px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: $radius-md;
  background: color-mix(in oklab, var(--tone) 15%, transparent);
  border: 1px solid color-mix(in oklab, var(--tone) 25%, $glass-stroke);
  color: var(--tone);
  overflow: hidden;
  transition: transform $t-base;

  &::after {
    content: '';
    position: absolute;
    inset: -2px;
    border-radius: inherit;
    pointer-events: none;
    background: radial-gradient(circle at 50% 50%, var(--tone), transparent 60%);
    opacity: 0;
    filter: blur(14px);
    transition: opacity $t-smooth;
  }

  svg {
    position: relative;
    z-index: 1;
  }
}

.corner-arrow {
  color: $text-muted;
  opacity: 0.55;
  transition: transform $t-base, opacity $t-base;
}

.route-name {
  font-family: $font-display;
  font-size: 15px;
  font-weight: 700;
  color: $text-primary;
  letter-spacing: -0.02em;
}

.route-desc {
  font-size: 12.5px;
  line-height: 1.55;
  color: $text-secondary;
  margin-bottom: 4px;
}

.route-meta {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.meta-label {
  font-size: 9.5px;
  font-weight: 700;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: $text-muted;
}

.meta-code {
  font-family: $font-mono;
  font-size: 11.5px;
  color: $text-code;
  word-break: break-all;
  line-height: 1.45;
  padding: 6px 10px;
  border-radius: $radius-sm;
  background: rgba(var(--bg-base-rgb), 0.38);
  border: 1px solid $glass-stroke;
}

.meta-code--ex {
  color: color-mix(in oklab, $accent-strong 70%, $text-secondary);
}

@media (max-width: 600px) {
  .routes-grid { grid-template-columns: 1fr; }
}
</style>
