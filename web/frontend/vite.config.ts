import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

//
// https://vite.dev/config/.
//
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    proxy: {
      '/ws': {
        target: 'ws://localhost:8080',
        ws: true,
      },
      '/api': {
        target: 'http://localhost:8080',
      },
    },
  },
  build: {
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      output: {
        manualChunks: {
          //
          // Split vendor libraries into separate chunks for better caching.
          //
          'vendor-react': ['react', 'react-dom', 'react-router-dom'],
          'vendor-flow': ['@xyflow/react', '@dagrejs/dagre'],
          'vendor-ui': ['lucide-react', '@xterm/xterm', '@xterm/addon-fit'],
          'vendor-markdown': ['react-markdown', 'remark-gfm'],
          'vendor-code': ['prism-react-renderer'],
        },
      },
    },
  },
})
