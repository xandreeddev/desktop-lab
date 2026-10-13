import { defineConfig } from 'astro/config';
import { execFileSync } from 'node:child_process';

function gitRef(args) {
  try {
    return execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim() || undefined;
  } catch {
    return undefined;
  }
}

const sourceRef = process.env.SITE_SOURCE_REF
  || gitRef(['symbolic-ref', '--quiet', '--short', 'HEAD'])
  || gitRef(['rev-parse', 'HEAD'])
  || 'main';

export default defineConfig({
  site: process.env.SITE_URL || 'https://xandreeddev.github.io',
  base: process.env.SITE_BASE_PATH || '/',
  output: 'static',
  devToolbar: { enabled: false },
  vite: { define: { 'import.meta.env.SITE_SOURCE_REF': JSON.stringify(sourceRef) } },
});
