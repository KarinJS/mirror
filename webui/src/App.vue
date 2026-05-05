<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Monitor, Sun, Moon, Languages, Settings2 } from 'lucide-vue-next'
import cnFlag from 'flag-icons/flags/4x3/cn.svg'
import usFlag from 'flag-icons/flags/4x3/us.svg'
import GlassBackdrop from './components/GlassBackdrop.vue'
import HeroSection from './components/HeroSection.vue'
import RouteList from './components/RouteList.vue'
import FooterSection from './components/FooterSection.vue'
import {
  type LocalePref, type LocaleKey,
  detectSystemLocale, readPref, writePref, resolveLocale, SUPPORTED_LOCALES,
} from './i18n'

type ThemePreference = 'system' | 'light' | 'dark'
type ThemeMode = 'dark' | 'light'

const STORAGE_KEY = 'mirror-theme'
const THEME_CYCLE: ThemePreference[] = ['system', 'light', 'dark']

const { t, locale } = useI18n()

/* ── theme ───────────────────────────────────── */
const theme = ref<ThemePreference>('system')
const systemTheme = ref<ThemeMode>('light')

const activeTheme = computed<ThemeMode>(() =>
  theme.value === 'system' ? systemTheme.value : theme.value
)

function getThemeIcon(pref: ThemePreference) {
  return { system: Monitor, light: Sun, dark: Moon }[pref]
}

let root: HTMLElement | null = null
let mediaQuery: MediaQueryList | null = null

function applyTheme() {
  if (!root) return
  root.dataset.theme = activeTheme.value
  window.localStorage.setItem(STORAGE_KEY, theme.value)
}

function setThemePref(pref: ThemePreference) {
  theme.value = pref
  applyTheme()
}

function syncSystemTheme(matches: boolean) {
  systemTheme.value = matches ? 'dark' : 'light'
  if (theme.value === 'system') applyTheme()
}
function handleSystemThemeChange(e: MediaQueryListEvent) { syncSystemTheme(e.matches) }

/* ── i18n ────────────────────────────────────── */
const localePref = ref<LocalePref>('auto')
const settingsOpen = ref(false)

function applyLocale() {
  locale.value = resolveLocale(localePref.value)
  writePref(localePref.value)
  document.documentElement.lang = locale.value
}

function setLocale(p: LocalePref) {
  localePref.value = p
  applyLocale()
}

function handleSystemLangChange() {
  if (localePref.value === 'auto') applyLocale()
}

function onDocClick(e: MouseEvent) {
  const tgt = e.target as HTMLElement
  if (!tgt.closest('.settings-pop')) settingsOpen.value = false
}

watch(activeTheme, () => {
  /* nothing — applyTheme already updates dataset */
})

onMounted(() => {
  root = document.documentElement
  mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
  syncSystemTheme(mediaQuery.matches)

  const stored = window.localStorage.getItem(STORAGE_KEY)
  theme.value = stored === 'system' || stored === 'light' || stored === 'dark' ? stored : 'system'
  applyTheme()
  mediaQuery.addEventListener('change', handleSystemThemeChange)

  localePref.value = readPref()
  applyLocale()
  // re-evaluate on system language change (Chrome fires storage event for languages on some systems)
  window.addEventListener('languagechange', handleSystemLangChange)
  document.addEventListener('click', onDocClick)
})

onUnmounted(() => {
  mediaQuery?.removeEventListener('change', handleSystemThemeChange)
  window.removeEventListener('languagechange', handleSystemLangChange)
  document.removeEventListener('click', onDocClick)
})

const isAutoActive = computed(() => localePref.value === 'auto')
const detectedLocale = computed<LocaleKey>(() => detectSystemLocale())

// 国旗映射： locale key → flag-icons country code
const LOCALE_FLAGS: Record<string, string> = {
  'zh-CN': 'cn',
  'en': 'us',
}
const FLAG_SRCS: Record<string, string> = {
  'cn': cnFlag,
  'us': usFlag,
}
</script>

<template>
  <div class="page">
    <GlassBackdrop />

    <header class="topbar">
      <a class="brand" href="/" :aria-label="t('brand.title') + t('brand.accent')">
        <img src="/logo.png" alt="Karin" class="brand-logo" />
        <span class="brand-title">{{ t('brand.title') }}<span class="brand-accent">{{ t('brand.accent') }}</span></span>
      </a>

      <div class="actions">
        <div class="settings-pop">
          <button
            class="chip-btn"
            :aria-label="t('topbar.settings')"
            :title="t('topbar.settings')"
            @click.stop="settingsOpen = !settingsOpen"
          >
            <Settings2 :size="15" :stroke-width="1.8" />
          </button>

          <Transition name="pop">
            <div v-if="settingsOpen" class="settings-menu" role="menu">
              <!-- ── 外观模式（放上面） ── -->
              <div class="menu-group">
                <span class="menu-label">{{ t('topbar.theme.label') }}</span>
                <button
                  v-for="tPref in THEME_CYCLE"
                  :key="tPref"
                  role="menuitem"
                  class="menu-item"
                  :class="{ active: theme === tPref }"
                  @click="setThemePref(tPref)"
                >
                  <component :is="getThemeIcon(tPref)" :size="14" />
                  <span>{{ t(`topbar.theme.${tPref}`) }}</span>
                </button>
              </div>

              <div class="menu-divider"></div>

              <!-- ── 切换语言（放下面，含国旗） ── -->
              <div class="menu-group">
                <span class="menu-label">{{ t('topbar.lang.label') }}</span>
                <button
                  role="menuitem"
                  class="menu-item"
                  :class="{ active: isAutoActive }"
                  @click="setLocale('auto')"
                >
                  <component :is="Languages" :size="14" />
                  <span>{{ t('topbar.lang.auto') }}</span>
                  <span class="hint">{{ t(`topbar.lang.${detectedLocale}`) }}</span>
                </button>
                <button
                  v-for="loc in SUPPORTED_LOCALES"
                  :key="loc"
                  role="menuitem"
                  class="menu-item"
                  :class="{ active: localePref === loc }"
                  @click="setLocale(loc)"
                >
                  <img class="flag-icon" :src="FLAG_SRCS[LOCALE_FLAGS[loc] ?? loc]" />
                  <span>{{ t(`topbar.lang.${loc}`) }}</span>
                </button>
              </div>
            </div>
          </Transition>
        </div>
      </div>
    </header>

    <main class="page-main">
      <HeroSection />

      <div class="page-inner">
        <RouteList />
      </div>

      <FooterSection />
    </main>
  </div>
</template>

<style lang="scss" scoped>
@use './styles/variables' as *;

.page {
  position: relative;
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  isolation: isolate;
}

/* ── topbar ─────────────────────────────────── */
.topbar {
  position: sticky;
  top: 0;
  z-index: 30;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px clamp(20px, 4vw, 36px);
  background: linear-gradient(
    to bottom,
    rgba(var(--bg-base-rgb), 0.55),
    rgba(var(--bg-base-rgb), 0.18) 70%,
    transparent
  );
  backdrop-filter: blur(20px) saturate(180%);
  -webkit-backdrop-filter: blur(20px) saturate(180%);
}

.brand {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  text-decoration: none;
  transition: transform $t-fast;

  &:hover { transform: translateY(-1px); }
}

.brand-logo {
  width: 38px;
  height: 38px;
  border-radius: 50%;
  object-fit: cover;
  border: 1px solid color-mix(in oklab, $accent-strong 14%, $glass-stroke);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.22),
    0 12px 28px rgba(15, 23, 42, 0.14);
}

.brand-title {
  font-family: $font-display;
  font-size: 15.5px;
  font-weight: 700;
  letter-spacing: -0.025em;
  color: $text-primary;
}

.brand-accent {
  color: color-mix(in oklab, $accent-strong 82%, white 18%);
}

/* ── action chips ───────────────────────────── */
.actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.chip-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  padding: 0;
  border: 1px solid color-mix(in oklab, $accent-strong 10%, $glass-stroke);
  border-radius: $radius-pill;
  background: color-mix(in oklab, $bg-surface 96%, transparent);
  backdrop-filter: blur(20px) saturate(180%);
  -webkit-backdrop-filter: blur(20px) saturate(180%);
  color: $text-secondary;
  font-size: 11.5px;
  font-weight: 600;
  letter-spacing: 0.04em;
  cursor: pointer;
  transition: color $t-fast, border-color $t-fast, background $t-fast, transform $t-fast;
  box-shadow: inset 0 1px 0 $glass-highlight, 0 8px 18px rgba(3, 16, 32, 0.08);

  &:hover {
    color: $text-primary;
    border-color: color-mix(in oklab, $accent-strong 18%, $border-hover);
    background: $bg-raised;
    transform: translateY(-1px);
  }
  &:active { transform: translateY(0) scale(0.97); }
  &:focus-visible { @include focus-ring; }

  svg { color: color-mix(in oklab, $accent-strong 70%, $text-secondary); }
}

/* ── settings menu ──────────────────────────── */
.settings-pop {
  position: relative;
}

.settings-menu {
  position: absolute;
  top: calc(100% + 8px);
  right: 0;
  min-width: 220px;
  display: flex;
  flex-direction: column;
  padding: 8px;
  border: 1px solid $glass-stroke;
  border-radius: $radius-lg;
  background:
    linear-gradient(140deg, rgba(255, 255, 255, 0.10) 0%, transparent 38%),
    rgba(var(--bg-base-rgb), 0.75);
  backdrop-filter: blur(28px) saturate(180%);
  -webkit-backdrop-filter: blur(28px) saturate(180%);
  box-shadow: $shadow-strong, inset 0 1px 0 $glass-highlight;
  z-index: 40;
}

.menu-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.menu-label {
  padding: 6px 12px 4px;
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: $text-muted;
}

.menu-divider {
  height: 1px;
  background: $glass-stroke;
  margin: 6px 4px;
}

.menu-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  border: 0;
  border-radius: $radius-md;
  background: transparent;
  color: $text-secondary;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: background $t-fast, color $t-fast;

  svg { color: $text-muted; flex-shrink: 0; }

  .hint {
    margin-left: auto;
    font-size: 10.5px;
    color: $text-muted;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .spacer { width: 14px; flex-shrink: 0; }

  // 国旗 icon 对齐 lucide 图标尺寸
  .flag-icon {
    width: 18px;
    height: 14px;
    flex-shrink: 0;
    border-radius: 2px;
    object-fit: cover;
    display: block;
  }

  &:hover { background: $bg-hover; color: $text-primary; svg { color: $text-primary; } }
  &.active {
    background: $accent-dim;
    color: $accent-strong;
    svg { color: $accent-strong; }
    .hint { color: $accent-strong; opacity: 0.8; }
  }
}

.pop-enter-active,
.pop-leave-active { transition: all 200ms cubic-bezier(0.32, 0.72, 0, 1); transform-origin: top right; }
.pop-enter-from   { opacity: 0; transform: translateY(-4px) scale(0.96); }
.pop-leave-to     { opacity: 0; transform: translateY(-4px) scale(0.96); }

.icon-spin-enter-active,
.icon-spin-leave-active { transition: all 280ms cubic-bezier(0.4, 0, 0.2, 1); }
.icon-spin-enter-from   { opacity: 0; transform: rotate(-120deg) scale(0.5); }
.icon-spin-leave-to     { opacity: 0; transform: rotate(120deg)  scale(0.5); }

/* ── main column ────────────────────────────── */
.page-main {
  position: relative;
  z-index: 1;
  width: 100%;
  display: flex;
  flex-direction: column;
  flex: 1;
}

.page-inner {
  width: 100%;
  max-width: 1180px;
  margin: 0 auto;
  padding: clamp(32px, 6vh, 64px) 0 clamp(40px, 6vh, 64px);
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: clamp(40px, 7vh, 72px);
}

@media (max-width: 640px) {
  .topbar { padding: 12px 16px; }
  .page-inner { padding: 24px 0 32px; gap: 36px; }
}
</style>
