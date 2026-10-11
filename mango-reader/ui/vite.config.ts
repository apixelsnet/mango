import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

// Relative asset paths, so the built UI works at the root and under a sub-path alike.
export default defineConfig({
  base: './',
  plugins: [svelte()],
  build: { outDir: 'dist', emptyOutDir: true, target: 'es2022' },
  // `npm run dev` talks to a Mango running on its default port.
  server: { proxy: { '/api': 'http://127.0.0.1:8787' } },
});
