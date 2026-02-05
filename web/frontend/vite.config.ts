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
    //
    // Internal tool, slightly larger chunks are acceptable.
    //
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      output: {
        manualChunks: {
          //
          // Split vendor libraries into separate chunks for better caching.
          //
          'vendor-react': ['react', 'react-dom', 'react-router-dom'],
          'vendor-flow': ['@xyflow/react'],
          'vendor-ui': ['lucide-react', '@xterm/xterm', '@xterm/addon-fit'],
          'vendor-markdown': ['react-markdown', 'remark-gfm'],
        },
      },
    },
  },
})
