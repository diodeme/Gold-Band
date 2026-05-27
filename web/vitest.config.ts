import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['web/src/**/*.test.ts'],
    environment: 'node',
  },
});
