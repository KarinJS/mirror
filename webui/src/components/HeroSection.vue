<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  AlertCircle,
  Check,
  Copy,
  ExternalLink,
  Gauge,
  Link2,
  ShieldCheck,
  Sparkles,
  X,
} from 'lucide-vue-next'
import { useLinkTransform } from '../composables/useLinkTransform'

const { t } = useI18n()
const { input, result, hasError, copied, clear, copyResult, openResult, fill } = useLinkTransform()

const EXAMPLES = [
  { key: 'release', url: 'https://github.com/NapNeko/NapCatQQ/releases/download/v4.18.0/NapCat.Framework.zip' },
  { key: 'raw', url: 'https://raw.githubusercontent.com/karinjs/karin/main/package.json' },
  { key: 'avatar', url: 'https://github.com/karinjs.png' },
  { key: 'unpkg', url: 'https://unpkg.com/karin/package.json' },
] as const

const highlights = [
  { key: 'allowlist', icon: ShieldCheck },
  { key: 'instant',   icon: Sparkles },
  { key: 'stable',    icon: Gauge },
] as const

const isShaking = ref(false)
let shakeTimer: ReturnType<typeof setTimeout> | undefined

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && hasError.value) triggerShake()
}
function triggerShake() {
  isShaking.value = false
  nextTick(() => {
    isShaking.value = true
    clearTimeout(shakeTimer)
    shakeTimer = setTimeout(() => (isShaking.value = false), 520)
  })
}
onUnmounted(() => clearTimeout(shakeTimer))

const convWrapClass = computed(() => ({
  'conv-wrap--ok':  !!result.value,
  'conv-wrap--err': hasError.value,
  shake:            isShaking.value,
}))
</script>

<template>
  <section class="hero">

    <!-- ── Atmospheric background blobs ──────────────────── -->
    <div class="hero-bg" aria-hidden="true">
      <div class="hero-bg__blob hero-bg__blob--a"></div>
      <div class="hero-bg__blob hero-bg__blob--b"></div>
    </div>

    <!-- ── Character illustration (decorative) ───────────── -->
    <figure class="hero-figure" aria-hidden="true">
      <img src="/karin.png" alt="" class="hero-figure__img" />
    </figure>

    <!-- ── Main content ───────────────────────────────────── -->
    <div class="hero-body"
      v-motion
      :initial="{ opacity: 0, y: 20 }"
      :enter="{ opacity: 1, y: 0, transition: { duration: 600, delay: 40 } }"
    >
      <!-- Kicker -->
      <div class="hero-kicker">
        <span class="kicker-pulse" aria-hidden="true"></span>
        <span>{{ t('hero.kicker') }}</span>
      </div>

      <!-- Title -->
      <h1 class="hero-title">
        <span class="hero-title__a">{{ t('hero.titleA') }}</span>
        <span class="hero-title__b">{{ t('hero.titleB') }}</span>
      </h1>

      <!-- Description -->
      <p class="hero-desc">{{ t('hero.desc') }}</p>

      <!-- Trait pills -->
      <div class="trait-row">
        <span v-for="item in highlights" :key="item.key" class="trait-pill">
          <component :is="item.icon" :size="13" :stroke-width="2" />
          {{ t(`hero.highlights.${item.key}.title`) }}
        </span>
      </div>

      <!-- ── Converter widget ────────────────────────────── -->
      <div class="converter">

        <!-- Input -->
        <label class="sr-only" for="mirror-input">{{ t('hero.inputPlaceholder') }}</label>
        <div class="conv-wrap" :class="convWrapClass">
          <Link2 :size="16" class="conv-icon" aria-hidden="true" />
          <input
            id="mirror-input"
            v-model="input"
            type="url"
            class="conv-input"
            :placeholder="t('hero.inputPlaceholder')"
            spellcheck="false"
            autocomplete="off"
            @keydown="onKeydown"
          />
          <button
            v-if="input"
            class="conv-clear"
            :aria-label="t('hero.clear')"
            @click="clear"
          >
            <X :size="14" :stroke-width="2.5" />
          </button>
        </div>

        <!-- Floating result / error — absolutely positioned, zero layout shift -->
        <div class="conv-float-panel">
          <Transition name="res-fade" mode="out-in">
            <div v-if="result" class="conv-result" :key="'res'">
              <div class="conv-result__meta">
                <span class="conv-badge" :style="{ '--rc': result.color }">
                  {{ t(`hero.rules.${result.label}`) }}
                </span>
                <code class="conv-result__url" :style="{ color: result.color }">{{ result.url }}</code>
              </div>
              <div class="conv-actions">
                <button class="conv-btn conv-btn--primary" :class="{ copied }" @click="copyResult">
                  <Check v-if="copied" :size="12" :stroke-width="2.5" />
                  <Copy v-else :size="12" />
                  {{ copied ? t('hero.copied') : t('hero.copy') }}
                </button>
                <button class="conv-btn" @click="openResult">
                  <ExternalLink :size="12" />
                  {{ t('hero.open') }}
                </button>
              </div>
            </div>

            <div v-else-if="hasError" class="conv-error" :key="'err'">
              <AlertCircle :size="14" :stroke-width="2" />
              {{ t('hero.error') }}
            </div>
          </Transition>
        </div>

        <!-- Example chips (hidden when result / error is active) -->
        <div class="conv-chips" :class="{ 'conv-chips--hidden': result || hasError }">
          <button
            v-for="ex in EXAMPLES"
            :key="ex.key"
            class="conv-chip"
            @click="fill(ex.url)"
          >{{ t(`hero.examples.${ex.key}`) }}</button>
        </div>

      </div><!-- /converter -->
    </div><!-- /hero-body -->

  </section>
</template>

<style lang="scss" scoped>
@use '../styles/variables' as *;

/* ═══════════════════════════════════════════════════════
   HERO — complete rewrite
   Layout: full-viewport section, illustration is absolute
   Content flows left-aligned inside .hero-body
═══════════════════════════════════════════════════════ */

/* ── Section shell ──────────────────────────────────── */
.hero {
  position: relative;
  width: 100%;
  height: calc(100svh - 72px);
  min-height: 580px;
  overflow: hidden;
  display: flex;
  align-items: center;
}

/* ── Atmospheric blobs ──────────────────────────────── */
.hero-bg {
  position: absolute;
  inset: 0;
  pointer-events: none;
  z-index: 0;
}

.hero-bg__blob {
  position: absolute;
  border-radius: 50%;
  filter: blur(90px);
  pointer-events: none;

  &--a {
    width: 55vw;
    height: 55vw;
    max-width: 700px;
    max-height: 700px;
    top: -20%;
    left: -8%;
    background: radial-gradient(
      circle,
      color-mix(in oklab, var(--accent-strong) 11%, transparent),
      transparent 68%
    );
  }

  &--b {
    width: 40vw;
    height: 40vw;
    max-width: 500px;
    max-height: 500px;
    bottom: -10%;
    right: 28%;
    background: radial-gradient(
      circle,
      color-mix(in oklab, var(--aurora-1) 55%, transparent),
      transparent 70%
    );
  }
}

/* ── Character illustration ─────────────────────────── */
.hero-figure {
  position: absolute;
  right: 0;
  top: 0;
  bottom: 0;
  width: 52%;
  margin: 0;
  pointer-events: none;
  z-index: 1;
  /* left fade + bottom fade via mask */
  mask-image:
    linear-gradient(to right, transparent 0%, rgba(0,0,0,.55) 22%, black 48%),
    linear-gradient(to top, transparent 0%, black 18%);
  mask-composite: intersect;
  -webkit-mask-image:
    linear-gradient(to right, transparent 0%, rgba(0,0,0,.55) 22%, black 48%),
    linear-gradient(to top, transparent 0%, black 18%);
  -webkit-mask-composite: destination-in;
}

.hero-figure__img {
  position: absolute;
  bottom: 0;
  right: 0;
  height: 96%;
  width: auto;
  max-width: 100%;
  object-fit: contain;
  object-position: bottom right;
  filter:
    drop-shadow(-24px 0 64px color-mix(in oklab, var(--accent-strong) 14%, transparent))
    drop-shadow(0 -8px 32px color-mix(in oklab, var(--aurora-1) 18%, transparent));
  animation: karin-float 5.5s ease-in-out infinite;
  transform-origin: bottom center;
}

@keyframes karin-float {
  0%, 100% { transform: translateY(0); }
  50%       { transform: translateY(-14px); }
}

/* ── Main content block ─────────────────────────────── */
.hero-body {
  position: relative;
  z-index: 2;
  width: 100%;
  max-width: 1200px;
  margin: 0 auto;
  padding: 0 clamp(20px, 5vw, 72px);
  display: flex;
  flex-direction: column;
  align-items: flex-start;
}

/* ── Kicker ─────────────────────────────────────────── */
.hero-kicker {
  display: inline-flex;
  align-items: center;
  gap: 9px;
  margin-bottom: 20px;
  color: $accent-strong;
  font-size: 11.5px;
  font-weight: 700;
  letter-spacing: 0.16em;
  text-transform: uppercase;
}

.kicker-pulse {
  display: block;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: $accent-strong;
  box-shadow: 0 0 0 3px color-mix(in oklab, var(--accent-strong) 22%, transparent);
  animation: kicker-pulse 2.4s ease-in-out infinite;
}

@keyframes kicker-pulse {
  0%, 100% { box-shadow: 0 0 0 3px color-mix(in oklab, var(--accent-strong) 22%, transparent); }
  50%       { box-shadow: 0 0 0 7px color-mix(in oklab, var(--accent-strong)  7%, transparent); }
}

/* ── Title ──────────────────────────────────────────── */
.hero-title {
  margin: 0 0 18px;
  display: flex;
  flex-direction: column;
  line-height: 1.0;
  letter-spacing: -0.045em;
  font-family: $font-display;
}

.hero-title__a {
  font-size: clamp(3rem, 5.8vw, 5.2rem);
  font-weight: 700;
  color: $text-primary;
}

.hero-title__b {
  font-size: clamp(3rem, 5.8vw, 5.2rem);
  font-weight: 800;
  @include text-gradient(
    color-mix(in oklab, var(--accent-strong) 90%, white),
    color-mix(in oklab, var(--accent-light) 85%, var(--accent-strong))
  );
}

/* ── Description ────────────────────────────────────── */
.hero-desc {
  margin: 0 0 26px;
  max-width: 46ch;
  font-size: clamp(1rem, 1.5vw, 1.1rem);
  line-height: 1.75;
  color: $text-secondary;
}

/* ── Trait pills ────────────────────────────────────── */
.trait-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 36px;
}

.trait-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 13px;
  border-radius: $radius-pill;
  border: 1px solid $border-accent;
  background: $accent-dim;
  color: $accent-strong;
  font-size: 12.5px;
  font-weight: 600;
  letter-spacing: -0.005em;
  transition: background $t-fast, box-shadow $t-fast;

  svg { flex-shrink: 0; opacity: .85; }

  &:hover {
    background: color-mix(in oklab, var(--accent-dim) 180%, transparent);
    box-shadow: 0 0 0 1px $border-accent;
  }
}

/* ── Converter widget ───────────────────────────────── */
.converter {
  width: 100%;
  max-width: 540px;
  display: flex;
  flex-direction: column;
  position: relative; // floated result panel anchor
}

/* Floating result panel — doesn't affect layout flow */
.conv-float-panel {
  position: absolute;
  top: calc(58px + 10px);
  left: 0;
  right: 0;
  z-index: 20;
}

/* Input row */
.conv-wrap {
  position: relative;
  display: flex;
  align-items: center;
  gap: 10px;
  height: 58px;
  padding: 0 8px 0 18px;
  border-radius: $radius-xl;
  border: 1.5px solid $border;
  background: $bg-raised;
  box-shadow: $shadow-soft, inset 0 1px 0 $glass-highlight;
  transition:
    border-color $t-fast,
    box-shadow   $t-fast;

  &:focus-within {
    border-color: color-mix(in oklab, var(--accent-strong) 45%, transparent);
    box-shadow:
      $shadow-soft,
      inset 0 1px 0 $glass-highlight,
      0 0 0 4px color-mix(in oklab, var(--accent-strong) 11%, transparent);

    .conv-icon { color: $accent-strong; }
  }

  &.shake { animation: conv-shake .5s cubic-bezier(.36,.07,.19,.97); }

  &.conv-wrap--err {
    border-color: color-mix(in oklab, var(--error) 40%, transparent);
    box-shadow: $shadow-soft, 0 0 0 3px color-mix(in oklab, var(--error) 9%, transparent);
  }

  &.conv-wrap--ok {
    border-color: color-mix(in oklab, var(--accent-strong) 38%, transparent);
    animation: none;
  }

  // ── flowing border light (::before) + breathing aura (::after) ──

  // shuttle beam — bright tight comet sweeping along the border
  &::before {
    content: '';
    position: absolute;
    inset: -1.5px;
    border-radius: $radius-xl;
    padding: 1.5px;
    background: linear-gradient(
      105deg,
      transparent 38%,
      transparent 44%,
      color-mix(in oklab, var(--accent-strong) 72%, transparent) 48%,
      color-mix(in oklab, var(--accent-light) 58%, transparent) 50%,
      white 51%,
      color-mix(in oklab, var(--accent-light) 48%, transparent) 52%,
      transparent 56%,
      transparent 62%
    );
    background-size: 200% 100%;
    -webkit-mask: linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0);
    mask: linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0);
    -webkit-mask-composite: xor;
    mask-composite: exclude;
    pointer-events: none;
    z-index: 0;
    opacity: 0.55;
    transition: opacity $t-smooth;
    animation: shuttle 3s linear infinite;
  }

  // ambient breathing aura — soft radial glow behind the input
  &::after {
    content: '';
    position: absolute;
    inset: -14px;
    border-radius: $radius-xl;
    background: radial-gradient(
      ellipse 80% 55% at center,
      color-mix(in oklab, var(--accent-strong) 13%, transparent) 0%,
      color-mix(in oklab, var(--accent-light) 6%, transparent) 35%,
      transparent 65%
    );
    pointer-events: none;
    z-index: -1;
    animation: aura 3.6s ease-in-out infinite;
  }

  // ── state tweaks ──

  &:hover::before { opacity: 0.72; }
  &:hover::after  { animation-duration: 2.4s; }

  &:focus-within {
    &::before { opacity: 0.95; animation-duration: 2s; }
    &::after  { animation-duration: 1.6s; }
  }

  &.conv-wrap--err {
    &::before { opacity: 0; animation: none; }
    &::after  { opacity: 0; animation: none; }
  }

  &.conv-wrap--ok {
    &::before { opacity: 0.3; animation-duration: 6s; }
    &::after  { opacity: 0.3; animation-duration: 5s; }
  }

  > * { position: relative; z-index: 1; }
}

@keyframes shuttle {
  from { background-position: 200% 0; }
  to   { background-position: 0% 0; }
}

@keyframes aura {
  0%, 100% { opacity: 0.3; transform: scale(1); }
  50%      { opacity: 0.65; transform: scale(1.025); }
}

@keyframes conv-shake {
  0%, 100% { transform: translateX(0); }
  20%       { transform: translateX(-6px); }
  40%       { transform: translateX(6px); }
  60%       { transform: translateX(-3px); }
  80%       { transform: translateX(3px); }
}

.conv-icon {
  flex-shrink: 0;
  color: $text-muted;
  transition: color $t-fast;
}

.conv-input {
  flex: 1;
  min-width: 0;
  border: 0;
  background: transparent;
  color: $text-primary;
  font-family: $font-mono;
  font-size: 13.5px;
  outline: none;

  &::placeholder {
    color: $text-muted;
    font-family: $font-sans;
    font-size: 13.5px;
  }
}

.conv-clear {
  flex-shrink: 0;
  width: 36px;
  height: 36px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: $radius-md;
  background: transparent;
  color: $text-muted;
  cursor: pointer;
  transition: background $t-fast, color $t-fast;

  &:hover { background: $bg-hover; color: $text-primary; }
}

/* Result panel */
.conv-result {
  margin-top: 10px;
  padding: 14px 16px;
  border-radius: $radius-lg;
  border: 1px solid color-mix(in oklab, var(--accent-strong) 18%, var(--glass-stroke));
  background: $bg-raised;
  box-shadow: $shadow-soft;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.conv-result__meta {
  display: flex;
  flex-direction: column;
  gap: 7px;
}

.conv-badge {
  display: inline-flex;
  align-items: center;
  width: fit-content;
  padding: 2px 9px;
  border-radius: $radius-pill;
  border: 1px solid color-mix(in srgb, var(--rc, var(--accent-strong)) 26%, transparent);
  background: color-mix(in srgb, var(--rc, var(--accent-strong)) 10%, transparent);
  color: var(--rc, var(--accent-strong));
  font-size: 10.5px;
  font-weight: 700;
  letter-spacing: 0.1em;
  text-transform: uppercase;
}

.conv-result__url {
  font-family: $font-mono;
  font-size: 12.5px;
  line-height: 1.55;
  word-break: break-all;
}

.conv-actions {
  display: flex;
  gap: 8px;
}

.conv-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 6px 14px;
  border: 1px solid $border;
  border-radius: $radius-pill;
  background: transparent;
  color: $text-secondary;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  transition: background $t-fast, color $t-fast, border-color $t-fast;

  &:hover { background: $bg-hover; color: $text-primary; border-color: $border-hover; }
  &:active { transform: scale(0.97); }

  &--primary {
    color: $accent-strong;
    border-color: $border-accent;
    background: $accent-dim;

    &:hover { background: color-mix(in oklab, var(--accent-dim) 170%, transparent); }

    &.copied {
      color: $green;
      border-color: color-mix(in oklab, var(--green) 26%, transparent);
      background: $green-dim;
    }
  }
}

/* Error state */
.conv-error {
  margin-top: 10px;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 16px;
  border-radius: $radius-lg;
  border: 1px solid $error-border;
  background: $error-dim;
  color: $error;
  font-size: 12.5px;
  line-height: 1.5;
}

/* Example chips */
.conv-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 14px;
  transition: opacity 180ms ease;

  &--hidden {
    opacity: 0;
    pointer-events: none;
  }
}

.conv-chip {
  padding: 4px 12px;
  border: 1px solid $border;
  border-radius: $radius-pill;
  background: rgba(var(--bg-base-rgb), .35);
  color: $text-secondary;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: background $t-fast, color $t-fast, border-color $t-fast;

  &:hover {
    background: $accent-dim;
    color: $accent-strong;
    border-color: $border-accent;
  }
}

/* ── Transitions ─────────────────────────────────────── */
.res-fade-enter-active { transition: opacity 180ms ease, transform 180ms $t-fast; }
.res-fade-leave-active { transition: opacity 130ms ease; }
.res-fade-enter-from   { opacity: 0; transform: translateY(5px); }
.res-fade-leave-to     { opacity: 0; }

/* ── A11y ────────────────────────────────────────────── */
.sr-only {
  position: absolute;
  width: 1px; height: 1px;
  padding: 0; margin: -1px;
  overflow: hidden;
  clip: rect(0,0,0,0);
  white-space: nowrap;
  border: 0;
}

/* ── Responsive ──────────────────────────────────────── */
@media (max-width: 860px) {
  .hero {
    height: auto;
    min-height: calc(100svh - 72px);
    align-items: flex-start;
    padding-top: 52px;
    padding-bottom: 48px;
  }

  .hero-figure {
    width: 72%;
    opacity: .3;
    mask-image: linear-gradient(to right, transparent, black 60%),
                linear-gradient(to top, transparent, black 20%);
    -webkit-mask-image: linear-gradient(to right, transparent, black 60%),
                        linear-gradient(to top, transparent, black 20%);
    mask-composite: intersect;
    -webkit-mask-composite: destination-in;
  }
}

@media (max-width: 520px) {
  .hero-figure { opacity: .18; width: 90%; }

  .hero-title__a,
  .hero-title__b { font-size: clamp(2.4rem, 10vw, 3rem); }

  .converter { max-width: 100%; }
}
</style>
