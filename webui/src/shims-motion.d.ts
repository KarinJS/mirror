/* Make `v-motion` directive recognized by vue-tsc template type-check. */
import type { Directive } from 'vue'

declare module 'vue' {
  interface GlobalDirectives {
    vMotion: Directive
  }
}

export {}
