import { defineConfig } from 'vite';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import react from '@vitejs/plugin-react';

const __dirname = fileURLToPath(new URL('.', import.meta.url));
import { readdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

function staticPwa() {
  return {
    name: 'rusted-utau-static-pwa',
    apply: 'build',
    async closeBundle() {
      const outDir = join(process.cwd(), 'dist');
      const files = await readdir(outDir, { recursive: true });
      const precache = files
        .filter((file) => typeof file === 'string' && /\.(html|js|css|wasm|svg|woff2?)$/.test(file))
        .map((file) => `./${file.replaceAll('\\\\', '/')}`)
        .concat('./manifest.webmanifest')
        .sort();
      await writeFile(join(outDir, 'manifest.webmanifest'), JSON.stringify({
        id: './',
        name: 'rusted Utau',
        short_name: 'rusted Utau',
        description: 'A vocal editor powered by the Rust project session.',
        start_url: './',
        scope: './',
        display: 'standalone',
        background_color: '#111217',
        theme_color: '#111217',
        icons: [{ src: './icon.svg', type: 'image/svg+xml', sizes: 'any', purpose: 'any' }],
      }, null, 2));
      await writeFile(join(outDir, 'icon.svg'), await readFile(join(process.cwd(), 'public/icon.svg')));
      await writeFile(join(outDir, 'sw.js'), `const CACHE = 'rusted-utau-shell-v1';\nconst ASSETS = ${JSON.stringify(precache)};\nself.addEventListener('install', (event) => event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(ASSETS)).then(() => self.skipWaiting())));\nself.addEventListener('activate', (event) => event.waitUntil(caches.keys().then((keys) => Promise.all(keys.filter((key) => key.startsWith('rusted-utau-shell-') && key !== CACHE).map((key) => caches.delete(key)))).then(() => self.clients.claim())));\nself.addEventListener('fetch', (event) => { const request = event.request; const url = new URL(request.url); if (request.method !== 'GET' || url.origin !== self.location.origin || url.pathname.includes('/api/') || url.pathname.endsWith('/projects')) return; if (request.mode === 'navigate') { event.respondWith(fetch(request).catch(() => caches.match('./index.html'))); return; } event.respondWith(caches.match(request).then((cached) => cached || fetch(request))); });\n`);
    },
  };
}

export default defineConfig({
  plugins: [react(), staticPwa()],
  base: './',
  // The Direct WASM backend imports the engine from ../../../web/pkg (outside
  // this project root), so the dev server must be allowed to serve those files.
  server: {
    fs: { allow: ['.', '../..'] },
  },
  // The clone's render preview imports the real `web/pool.js`, which starts
  // module Web Workers (`web/worker.js`). Rollup cannot code-split a worker
  // into an IIFE, so it has to be emitted as ES.
  worker: { format: 'es' },
  build: {
    rollupOptions: {
      // The OpenUtau-style editor is the primary app at `/`; keep explicit
      // clone and generic-workbench pages for comparison and internal review.
      input: {
        main: resolve(__dirname, 'index.html'),
        openutau: resolve(__dirname, 'openutau.html'),
      },
      output: {
        manualChunks(id) {
          if (!id.includes('/node_modules/')) return undefined;
          if (id.includes('/pixi.js/') || id.includes('/@pixi/')) return 'pixi-vendor';
          if (id.includes('/react/') || id.includes('/react-dom/') || id.includes('/scheduler/')) return 'react-vendor';
          if (id.includes('/dockview') || id.includes('/@floating-ui/')) return 'dockview-vendor';
          return undefined;
        },
      },
    },
  },
});
