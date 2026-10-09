import type { APIRoute } from 'astro';
import license from '../../../../lucent/apps/lucent-desktop/assets/OFL-GoogleSansFlex.txt?raw';
export const GET: APIRoute = () => new Response(license, {
  headers: { 'Content-Type': 'text/plain; charset=utf-8' },
});
