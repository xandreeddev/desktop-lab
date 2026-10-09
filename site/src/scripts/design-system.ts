import { tokens } from '../data/tokens';
const themed = document.querySelector<HTMLElement>('[data-design-theme]');
themed?.querySelectorAll<HTMLButtonElement>('[data-theme-choice]').forEach(button => {
  button.addEventListener('click', () => {
    const mode = button.dataset.themeChoice ?? 'dark';
    themed.dataset.designTheme = mode;
    themed.querySelectorAll('[data-theme-choice]').forEach(item => item.setAttribute('aria-pressed', String(item === button)));
    themed.querySelectorAll<HTMLElement>('[data-role-value]').forEach(value => {
      value.textContent = String(tokens.find(t => t.name === `theme.${mode}.${value.dataset.roleValue}`)?.value ?? '');
    });
  });
});
const explorer = document.querySelector<HTMLElement>('[data-token-explorer]');
const search = explorer?.querySelector<HTMLInputElement>('[data-token-search]');
const group = explorer?.querySelector<HTMLSelectElement>('[data-token-group]');
function filterTokens() {
  const term = search?.value.trim().toLowerCase() ?? '';
  const prefix = group?.value ?? 'theme';
  let visible = 0;
  explorer?.querySelectorAll<HTMLElement>('[data-token-name]').forEach(row => {
    const name = row.dataset.tokenName ?? '';
    row.hidden = !(name.includes(term) && (prefix === 'all' || name.startsWith(`${prefix}.`)));
    if (!row.hidden) visible++;
  });
  const count = explorer?.querySelector('[data-token-count]');
  if (count) count.textContent = `${visible} ${visible === 1 ? 'token' : 'tokens'}`;
  const empty = explorer?.querySelector<HTMLElement>('[data-token-empty]');
  if (empty) empty.hidden = visible > 0;
}
search?.addEventListener('input', filterTokens);
group?.addEventListener('change', filterTokens);
explorer?.querySelectorAll<HTMLButtonElement>('[data-token-copy]').forEach(button => {
  button.addEventListener('click', async () => {
    const status = explorer.querySelector('[data-token-status]');
    const value = button.dataset.tokenCopy ?? '';
    try {
      await navigator.clipboard.writeText(value);
      if (status) status.textContent = `Copied ${value}`;
    } catch { if (status) status.textContent = `Copy manually: ${value}`; }
  });
});
