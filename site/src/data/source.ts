// Local guides follow the working branch; detached CI/release builds use their
// actual commit. Source archives can supply SITE_SOURCE_REF or fall back to main.
// The build configuration resolves Git metadata; this helper stays pure.
const sourceRef = import.meta.env.SITE_SOURCE_REF || 'main';
export const sourceUrl = (path: string) =>
  `https://github.com/xandreeddev/desktop-lab/blob/${encodeURIComponent(sourceRef)}/${path}`;

/** Build-time excerpts fail to build if their source markers disappear. */
export function excerpt(source: string, start: string, end: string): string {
  const from = source.indexOf(start);
  const to = source.indexOf(end, from + start.length);
  if (from < 0 || to < 0) throw new Error(`Missing documentation excerpt: ${start}`);
  return source.slice(from, to).trim();
}
