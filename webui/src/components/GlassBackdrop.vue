<template>
  <div class="backdrop" aria-hidden="true">
    <div class="aurora aurora--1"></div>
    <div class="aurora aurora--2"></div>
    <div class="aurora aurora--3"></div>
    <div class="grid"></div>
    <div class="vignette"></div>
  </div>
</template>

<style lang="scss" scoped>
@use '../styles/variables' as *;

.backdrop {
  position: fixed;
  inset: 0;
  z-index: 0;
  overflow: hidden;
  pointer-events: none;
  background:
    radial-gradient(ellipse 88% 66% at 50% -8%, rgba(214, 239, 255, 0.08), transparent 58%),
    linear-gradient(180deg, rgba(255, 255, 255, 0.02), transparent 24%),
    radial-gradient(ellipse 70% 52% at 50% 100%, rgba(0, 0, 0, 0.14), transparent 70%),
    var(--bg-base);
}

.aurora {
  position: absolute;
  border-radius: 50%;
  filter: blur(138px) saturate(112%);
  will-change: transform;
  mix-blend-mode: screen;
  opacity: 0.68;
}

[data-theme='light'] .aurora {
  filter: blur(150px) saturate(106%);
  mix-blend-mode: normal;
  opacity: 0.52;
}

.aurora--1 {
  width: 60vw;
  height: 60vw;
  max-width: 920px;
  max-height: 920px;
  top: -26%;
  left: -16%;
  background: radial-gradient(circle at 30% 30%, $aurora-1, transparent 65%);
  animation: aurora-drift 42s ease-in-out infinite;
}

.aurora--2 {
  width: 55vw;
  height: 55vw;
  max-width: 860px;
  max-height: 860px;
  top: -4%;
  right: -16%;
  background: radial-gradient(circle at 70% 30%, $aurora-2, transparent 65%);
  animation: aurora-drift 48s ease-in-out -10s infinite reverse;
}

.aurora--3 {
  width: 48vw;
  height: 48vw;
  max-width: 720px;
  max-height: 720px;
  bottom: -28%;
  left: 12%;
  background: radial-gradient(circle at 50% 50%, $aurora-3, transparent 70%);
  animation: aurora-drift 54s ease-in-out -18s infinite;
}

/* fine grid pattern */
.grid {
  position: absolute;
  inset: 0;
  background-image:
    linear-gradient(to right,  $grid-color 1px, transparent 1px),
    linear-gradient(to bottom, $grid-color 1px, transparent 1px);
  background-size: 56px 56px;
  background-position: -1px -1px;
  mask-image: radial-gradient(ellipse 100% 80% at 50% 35%, #000 30%, transparent 80%);
  -webkit-mask-image: radial-gradient(ellipse 100% 80% at 50% 35%, #000 30%, transparent 80%);
  opacity: 0.16;
}

.vignette {
  position: absolute;
  inset: 0;
  background: radial-gradient(ellipse at 50% 50%, transparent 52%, rgba(0, 0, 0, 0.28) 100%);
  pointer-events: none;
}

[data-theme='light'] .vignette {
  background: radial-gradient(ellipse at 50% 50%, transparent 60%, rgba(44, 53, 63, 0.05) 100%);
}
</style>
