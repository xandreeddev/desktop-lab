// The page content is server-rendered; these are small interaction enhancements.
document.querySelectorAll<HTMLElement>('[data-tabs]').forEach((group) => {
  const tabs = Array.from(group.querySelectorAll<HTMLButtonElement>('[role="tab"]'));
  const select = (tab: HTMLButtonElement, moveFocus = false) => {
    tabs.forEach((item) => {
      const selected = item === tab;
      item.setAttribute('aria-selected', String(selected));
      item.tabIndex = selected ? 0 : -1;
      const panel = document.getElementById(item.getAttribute('aria-controls') ?? '');
      if (panel) panel.hidden = !selected;
    });
    if (moveFocus) tab.focus();
  };
  tabs.forEach((tab, index) => {
    tab.addEventListener('click', () => select(tab));
    tab.addEventListener('keydown', (event) => {
      const vertical = tab.parentElement?.getAttribute('aria-orientation') === 'vertical';
      const previous = vertical ? 'ArrowUp' : 'ArrowLeft';
      const next = vertical ? 'ArrowDown' : 'ArrowRight';
      let destination: number | undefined;
      if (event.key === previous) destination = (index - 1 + tabs.length) % tabs.length;
      if (event.key === next) destination = (index + 1) % tabs.length;
      if (event.key === 'Home') destination = 0;
      if (event.key === 'End') destination = tabs.length - 1;
      if (destination !== undefined) {
        event.preventDefault();
        select(tabs[destination], true);
      }
    });
  });
});

document.querySelectorAll<HTMLButtonElement>('[data-copy]').forEach((button) => {
  let reset: ReturnType<typeof setTimeout>;
  button.addEventListener('click', async () => {
    const status = button.closest('.code-block')?.querySelector('.copy-status');
    try {
      await navigator.clipboard.writeText(button.dataset.copy ?? '');
      button.textContent = 'Copied ✓';
      if (status) status.textContent = 'Code copied to clipboard.';
    } catch {
      button.textContent = 'Select code';
      if (status) status.textContent = 'Clipboard unavailable. The code has been selected for manual copying.';
      const code = button.closest('.code-block')?.querySelector('pre');
      if (code) {
        const range = document.createRange();
        range.selectNodeContents(code);
        const selection = window.getSelection();
        selection?.removeAllRanges();
        selection?.addRange(range);
      }
    }
    clearTimeout(reset);
    reset = setTimeout(() => { button.textContent = 'Copy'; }, 2400);
  });
});

document.querySelectorAll<HTMLElement>('[data-motion-demo]').forEach(demo => {
  demo.querySelector<HTMLButtonElement>('[data-motion-toggle]')?.addEventListener('click', event => {
    const button = event.currentTarget as HTMLButtonElement;
    const expanded = button.getAttribute('aria-expanded') !== 'true';
    button.setAttribute('aria-expanded', String(expanded));
    demo.classList.toggle('expanded', expanded);
    button.textContent = expanded ? 'Collapse to dock ↙' : 'Expand launcher ↗';
  });
});

if ('IntersectionObserver' in window) {
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      document.querySelectorAll<HTMLAnchorElement>('.guide-nav a').forEach((link) => {
        if (link.hash === `#${entry.target.id}`) link.setAttribute('aria-current', 'location');
        else link.removeAttribute('aria-current');
      });
    }
  }, { rootMargin: '-15% 0px -65% 0px' });
  document.querySelectorAll('main section[id]').forEach((section) => observer.observe(section));
}
