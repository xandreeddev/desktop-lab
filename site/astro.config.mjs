import { defineConfig } from 'astro/config';

export default defineConfig({
  site: process.env.SITE_URL || 'https://xandreeddev.github.io',
  base: process.env.SITE_BASE_PATH || '/',
  output: 'static',
  devToolbar: { enabled: false },
});
