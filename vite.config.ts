import { defineConfig } from 'vitest/config';
import { fileIconThemes } from './scripts/fileIconThemes';
import { glslGrammar } from './scripts/glslGrammar';

export default defineConfig({
  clearScreen: false,
  plugins: [fileIconThemes(), glslGrammar()],
  worker: { format: 'es', plugins: () => [glslGrammar()] },
  // Dependency prebundling bypasses Vite load hooks; keep grammar imports on the licensed path.
  optimizeDeps: { exclude: ['shiki', '@shikijs/langs'] },
  server: { host: '127.0.0.1', port: 1420, strictPort: true },
  test: {
    environment: 'jsdom',
    setupFiles: ['./tests/setup.ts'],
    server: { deps: { inline: ['shiki', '@shikijs/langs'] } },
    include: ['tests/unit/**/*.test.{ts,tsx}', 'tests/integration/**/*.test.{ts,tsx}'],
  },
});
