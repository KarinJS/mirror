import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { fontSubsetter } from 'rollup-plugin-font-subsetter'

const proxyTarget = process.env.MIRROR_PROXY_TARGET ?? 'http://127.0.0.1:7878'
const proxyRoutes = ['/gh', '/raw', '/avatar', '/unpkg', '/mirror', '/healthz', '/stats']

function stripWoff(): import('vite').Plugin {
  return {
    name: 'strip-woff',
    enforce: 'pre',
    transform(code, id) {
      if (!/\.css\b/.test(id)) return
      return code.replace(/,\s*url\([^)]*\.woff\)\s*format\(['"]woff['"]\)/g, '')
    },
  }
}

export default defineConfig({
  plugins: [stripWoff(), vue(), fontSubsetter()],
  server: {
    port: 5175,
    proxy: Object.fromEntries(proxyRoutes.map((route) => [route, proxyTarget])),
  },
})
