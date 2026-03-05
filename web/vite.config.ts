import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  base: '/memory/',
  plugins: [tailwindcss(), svelte()],
  server: {
    port: 5173,
    proxy: {
      '/memory/api': {
        target: 'http://localhost:3000',
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/memory\/api/, '/api'),
      },
    },
  },
});
