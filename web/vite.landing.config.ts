import { defineConfig, type Plugin } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { resolve } from 'path';
import { renameSync, existsSync } from 'fs';

function renameLandingHtml(): Plugin {
  return {
    name: 'rename-landing-html',
    closeBundle() {
      const from = resolve(__dirname, 'dist-landing/landing.html');
      const to = resolve(__dirname, 'dist-landing/index.html');
      if (existsSync(from)) {
        renameSync(from, to);
      }
    },
  };
}

export default defineConfig({
  base: '/',
  plugins: [tailwindcss(), svelte(), renameLandingHtml()],
  build: {
    outDir: 'dist-landing',
    emptyOutDir: true,
    rollupOptions: {
      input: 'landing.html',
    },
  },
});
