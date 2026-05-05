<script setup lang="ts">
import { computed } from 'vue'
import { Github } from 'lucide-vue-next'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()
const year = computed(() => new Date().getFullYear())
</script>

<template>
  <footer class="footer">
    <div class="footer-inner">
      <span class="footer-copy">{{ t('footer.copyright', { year }) }}</span>
      <span class="footer-sep">·</span>
      <a class="footer-link" href="https://github.com/karinjs" target="_blank" rel="noopener noreferrer">
        <Github :size="13" :stroke-width="1.8" />
        <span>GitHub</span>
      </a>
      <span class="footer-sep">·</span>
      <span class="footer-stack">{{ t('footer.stack') }}</span>
    </div>
  </footer>
</template>

<style lang="scss" scoped>
@use '../styles/variables' as *;

.footer {
  width: 100%;
  padding: 48px clamp(20px, 4vw, 36px) 32px;
}

.footer-inner {
  max-width: 1180px;
  margin: 0 auto;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  flex-wrap: wrap;
  font-size: 12px;
  color: $text-muted;
}

.footer-sep {
  color: color-mix(in oklab, $text-muted 50%, transparent);
}

.footer-link {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  color: $text-secondary;
  text-decoration: none;
  transition: color $t-fast;

  &:hover { color: $accent-strong; }
}

.footer-stack {
  color: $text-muted;
}

.footer-copy {
  color: $text-muted;
}

@media (max-width: 480px) {
  .footer { padding-top: 36px; padding-bottom: 24px; }
  .footer-inner { gap: 6px; font-size: 11px; }
}
</style>
