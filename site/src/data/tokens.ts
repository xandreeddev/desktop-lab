import source from '../../../design/tokens.json';
export interface Token { name: string; type: string; raw: string | number | number[]; value: string | number | number[]; }
interface Node { [key: string]: Node | string | number | number[]; }
const raw: Record<string, { type: string; value: Token['raw'] }> = {};
function flatten(node: Node, path = '') {
  for (const [key, child] of Object.entries(node)) {
    if (key.startsWith('$')) continue;
    const name = path ? `${path}.${key}` : key;
    const token = child as Node;
    if ('$value' in token) raw[name] = { type: String(token.$type), value: token.$value as Token['raw'] };
    else flatten(token, name);
  }
}
flatten(source as unknown as Node);
function resolve(name: string, chain: string[] = []): Token['value'] {
  if (!raw[name] || chain.includes(name)) throw new Error(`Invalid token reference: ${name}`);
  const value = raw[name].value;
  return typeof value === 'string' && value.startsWith('{') ? resolve(value.slice(1, -1), [...chain, name]) : value;
}
export const tokens: Token[] = Object.entries(raw).map(([name, token]) => ({ name, type: token.type, raw: token.value, value: resolve(name) }));
export const displayValue = (token: Token) => {
  if (token.type === 'bezier') return `cubic-bezier(${(token.value as number[]).join(', ')})`;
  return `${token.value}${token.type === 'dimension' ? 'px' : token.type === 'duration' ? 'ms' : ''}`;
};
export const cssName = (name: string) => `--${name.replaceAll('.', '-').replaceAll('_', '-')}`;
export const rustName = (name: string) => { const parts = name.split('.'); return `lucent_design::${parts.slice(0,-1).join('::')}::${parts.at(-1)?.toUpperCase()}`; };
