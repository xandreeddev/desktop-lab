import type { APIRoute } from 'astro';
import measurements from '../../../../reports/measurements/lucent-framework-idle-600.json';
export const GET: APIRoute = () => new Response(JSON.stringify(measurements, null, 2), {
  headers: { 'Content-Type': 'application/json; charset=utf-8' },
});
