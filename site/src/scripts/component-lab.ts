export interface LabVariant {
  id: string;
  palette: string;
  state: string;
  density: { numerator: number; denominator: number };
  viewport: { id: string; width: number; height: number };
  image: { path: string; width: number; height: number; sha256: string };
  time_ms?: number;
}
export interface LabStory {
  id: string;
  family: string;
  title: string;
  description: string;
  recipe: string;
  source: { path: string; symbol: string };
  tokens: string[];
  variants: LabVariant[];
}
export interface LabManifest {
  schema_version: number;
  renderer: { engine: string; source_commit: string; token_sha256: string; scope: string; inputs: Record<string, string> };
  palettes: { id: string; name: string }[];
  stories: LabStory[];
}
type Dimension = 'palette' | 'state' | 'scale' | 'viewport';
type Choice = Record<Dimension, string>;
const dimensions: Dimension[] = ['palette', 'state', 'scale', 'viewport'];
const scaleKey = (variant: LabVariant) => String(variant.density.numerator / variant.density.denominator);
export const variantChoice = (variant: LabVariant): Choice => ({ palette: variant.palette, state: variant.state, scale: scaleKey(variant), viewport: variant.viewport.id });
export function chooseVariant(story: LabStory, requested: Partial<Choice>): LabVariant | undefined {
  // A new story may not have the old story's states or sizes. Keep the greatest
  // number of real requested dimensions; never manufacture an absent variant.
  return story.variants.reduce<LabVariant | undefined>((best, candidate) => {
    const score = (variant: LabVariant) => dimensions.filter(key => variantChoice(variant)[key] === requested[key]).length;
    return !best || score(candidate) > score(best) ? candidate : best;
  }, undefined);
}
export function matchesStory(story: LabStory, query: string, family: string): boolean {
  const text = `${story.title} ${story.family} ${story.description} ${story.tokens.join(' ')}`.toLowerCase();
  return (family === 'all' || story.family === family) && query.trim().toLowerCase().split(/\s+/).every(word => text.includes(word));
}
function safeImagePath(path: string): boolean {
  return /^images\/[a-zA-Z0-9_./-]+\.png$/.test(path) && !path.split('/').includes('..');
}
const humanize = (value: string) => value.replaceAll(/[-_]/g, ' ').replace(/^./, letter => letter.toUpperCase());

function initializeLab(root: HTMLElement): void {
  const element = <T extends HTMLElement>(selector: string) => {
    const match = root.querySelector<T>(selector);
    if (!match) throw new Error(`Missing component explorer control: ${selector}`);
    return match;
  };
  const feedback = element<HTMLElement>('[data-lab-status]');
  try {
    const manifest = JSON.parse(element<HTMLScriptElement>('[data-lab-manifest]').textContent ?? '') as LabManifest;
    if (manifest.schema_version !== 1 || !Array.isArray(manifest.stories) || !manifest.stories.length) {
      throw new Error('No supported native captures are available. Regenerate the component lab manifest.');
    }
    const search = element<HTMLInputElement>('[data-lab-search]');
    const family = element<HTMLSelectElement>('[data-lab-family]');
    const storyList = element<HTMLElement>('[data-lab-story-list]');
    const storyButtons = Array.from(storyList.querySelectorAll<HTMLButtonElement>('[data-lab-story]'));
    const selectors = Object.fromEntries(dimensions.map(key => [key, element<HTMLSelectElement>(`[data-lab-dimension="${key}"]`)])) as Record<Dimension, HTMLSelectElement>;
    const zoom = element<HTMLSelectElement>('[data-lab-zoom]');
    const stage = element<HTMLElement>('[data-lab-stage]');
    const canvas = element<HTMLElement>('[data-lab-canvas]');
    const image = element<HTMLImageElement>('[data-lab-image]');
    const imageStatus = element<HTMLElement>('[data-lab-image-status]');
    const share = element<HTMLButtonElement>('[data-lab-share]');
    const download = element<HTMLAnchorElement>('[data-lab-download]');
    const assetRoot = root.dataset.assetRoot ?? '';
    let story = manifest.stories[0];
    let variant = story.variants[0];
    let imageGeneration = 0;
    if (!variant) throw new Error('This story has no native captures. Regenerate the component lab manifest.');
    let displayedVariant = variant;
    let hasGoodImage = image.complete && image.naturalWidth > 0;

    const paletteName = (id: string) => manifest.palettes.find(palette => palette.id === id)?.name ?? humanize(id);
    const sourceLink = (path: string) => {
      const base = root.dataset.sourceRoot ?? '';
      const pinned = /^[a-f0-9]{40}$/.test(manifest.renderer.source_commit)
        ? base.replace(/\/blob\/[^/]+\//, `/blob/${manifest.renderer.source_commit}/`)
        : base;
      return pinned + path;
    };
    const setText = (selector: string, text: string) => { element<HTMLElement>(selector).textContent = text; };
    const markStory = () => {
      storyButtons.forEach(button => {
        const selected = button.dataset.labStory === story.id;
        button.setAttribute('aria-pressed', String(selected));
        button.tabIndex = selected ? 0 : -1;
      });
    };
    const filterStories = () => {
      let visible = 0;
      storyButtons.forEach(button => {
        const candidate = manifest.stories.find(item => item.id === button.dataset.labStory);
        button.hidden = !candidate || !matchesStory(candidate, search.value, family.value);
        if (!button.hidden) visible++;
      });
      setText('[data-lab-story-count]', `${visible} ${visible === 1 ? 'story' : 'stories'}`);
      element<HTMLElement>('[data-lab-empty]').hidden = visible !== 0;
      // Keep the already selected preview when filtering hides its story. The
      // remaining list still has a keyboard entry point and no implicit action.
      const entry = storyButtons.find(button => !button.hidden && button.dataset.labStory === story.id)
        ?? storyButtons.find(button => !button.hidden);
      storyButtons.forEach(button => { button.tabIndex = button === entry ? 0 : -1; });
    };
    const updateUrl = () => {
      const url = new URL(window.location.href);
      url.searchParams.set('story', story.id);
      const choice = variantChoice(variant);
      dimensions.forEach(key => url.searchParams.set(key, choice[key]));
      url.searchParams.set('zoom', zoom.value);
      window.history.replaceState(null, '', url);
    };
    const resizeImage = () => {
      const style = window.getComputedStyle(canvas);
      const insetX = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
      const insetY = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);
      let factor = Number(zoom.value);
      if (zoom.value === 'fit') factor = Math.min(1, Math.max(0, stage.clientWidth - insetX) / displayedVariant.viewport.width, Math.max(0, stage.clientHeight - insetY) / displayedVariant.viewport.height);
      const width = zoom.value === 'pixels' ? displayedVariant.image.width : displayedVariant.viewport.width * factor;
      const height = zoom.value === 'pixels' ? displayedVariant.image.height : displayedVariant.viewport.height * factor;
      image.style.width = `${width}px`;
      image.style.height = `${height}px`;
    };
    const renderSelectors = () => {
      const selected = variantChoice(variant);
      dimensions.forEach(key => {
        const selector = selectors[key];
        selector.replaceChildren();
        const values = [...new Set(story.variants.map(candidate => variantChoice(candidate)[key]))];
        values.forEach(value => {
          const option = document.createElement('option');
          option.value = value;
          option.textContent = key === 'palette' ? paletteName(value) : key === 'scale' ? `${value}×` : humanize(value);
          option.selected = value === selected[key];
          option.disabled = !story.variants.some(candidate => {
            const choice = variantChoice(candidate);
            return choice[key] === value && dimensions.every(other => other === key || choice[other] === selected[other]);
          });
          if (option.disabled) option.textContent += ' · not captured';
          selector.append(option);
        });
        selector.disabled = values.length < 2;
      });
    };
    const loadImage = async () => {
      const generation = ++imageGeneration;
      imageStatus.hidden = false;
      imageStatus.textContent = 'Loading the selected native capture; the previous image stays visible until it is ready.';
      stage.setAttribute('aria-busy', 'true');
      download.hidden = true;
      try {
        if (!safeImagePath(variant.image.path)) throw new Error('The capture path is invalid. Regenerate the component lab manifest.');
        const selected = variant;
        const url = assetRoot + selected.image.path;
        const pending = new Image();
        pending.src = url;
        await pending.decode();
        if (generation !== imageGeneration) return;
        if (pending.naturalWidth !== selected.image.width || pending.naturalHeight !== selected.image.height) {
          throw new Error('This capture does not match its manifest dimensions. Regenerate the component lab images.');
        }
        image.src = url;
        image.width = selected.image.width;
        image.height = selected.image.height;
        image.alt = `${story.title}, ${paletteName(selected.palette)}, ${humanize(selected.state)}, ${scaleKey(selected)}× density; native renderer capture`;
        displayedVariant = selected;
        hasGoodImage = true;
        resizeImage();
        image.hidden = false;
        imageStatus.hidden = true;
        download.href = url;
        download.download = `${story.id.replaceAll('/', '-')}-${selected.id}.png`;
        download.hidden = false;
      } catch (error) {
        if (generation !== imageGeneration) return;
        hasGoodImage ||= image.complete && image.naturalWidth === displayedVariant.image.width && image.naturalHeight === displayedVariant.image.height;
        image.hidden = !hasGoodImage;
        imageStatus.textContent = `The selected capture could not be loaded or does not match its manifest.${hasGoodImage ? ' The previous capture is still shown.' : ''} Choose another variant or regenerate the component lab images.`;
      } finally {
        if (generation === imageGeneration) stage.setAttribute('aria-busy', 'false');
      }
    };
    const render = () => {
      markStory();
      renderSelectors();
      setText('[data-lab-story-family]', story.family);
      setText('[data-lab-title]', story.title);
      setText('[data-lab-description]', story.description);
      setText('[data-lab-recipe]', story.recipe);
      setText('[data-lab-dimensions]', `${variant.viewport.width} × ${variant.viewport.height} logical · ${variant.image.width} × ${variant.image.height} captured pixels`);
      const source = element<HTMLAnchorElement>('[data-lab-source]');
      source.href = sourceLink(story.source.path);
      source.textContent = `${story.source.symbol} ↗`;
      const tokens = element<HTMLElement>('[data-lab-tokens]');
      tokens.replaceChildren(...story.tokens.map(token => {
        const item = document.createElement('li');
        const code = document.createElement('code');
        code.textContent = token;
        item.append(code);
        return item;
      }));
      const sourceLabel = manifest.renderer.source_commit || 'source revision not recorded';
      const tokenLabel = manifest.renderer.token_sha256 || 'token digest not recorded';
      setText('[data-lab-provenance]', `${manifest.renderer.engine} · ${manifest.renderer.scope}. Source: ${sourceLabel}. Tokens SHA-256: ${tokenLabel}. Image SHA-256: ${variant.image.sha256}.`);
      feedback.textContent = `${story.title} · ${paletteName(variant.palette)} · ${humanize(variant.state)} · ${scaleKey(variant)}× · ${humanize(variant.viewport.id)}. Select a different capture above; use the native lab for input.`;
      void loadImage();
    };
    const readUrl = () => {
      const params = new URL(window.location.href).searchParams;
      const requestedStory = manifest.stories.find(candidate => candidate.id === params.get('story'));
      story = requestedStory ?? manifest.stories[0];
      variant = chooseVariant(story, Object.fromEntries(dimensions.map(key => [key, params.get(key) ?? '']))) ?? story.variants[0];
      if (!variant) throw new Error('The selected story has no captures.');
      zoom.value = ['fit', '1', '2', 'pixels'].includes(params.get('zoom') ?? '') ? params.get('zoom')! : 'fit';
      search.value = '';
      family.value = 'all';
      render();
      filterStories();
    };
    const selectStory = (id: string) => {
      const candidate = manifest.stories.find(item => item.id === id);
      if (!candidate?.variants.length) return;
      variant = chooseVariant(candidate, variantChoice(variant))!;
      story = candidate;
      render();
      filterStories();
      updateUrl();
    };
    storyButtons.forEach(button => button.addEventListener('click', () => selectStory(button.dataset.labStory ?? '')));
    storyList.addEventListener('keydown', event => {
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
      const visible = storyButtons.filter(button => !button.hidden);
      if (!visible.length) return;
      const current = visible.indexOf(document.activeElement as HTMLButtonElement);
      const index = event.key === 'Home' ? 0 : event.key === 'End' ? visible.length - 1 : (current + (event.key === 'ArrowDown' ? 1 : -1) + visible.length) % visible.length;
      event.preventDefault();
      const destination = visible[index];
      selectStory(destination.dataset.labStory ?? '');
      destination.focus();
    });
    dimensions.forEach(key => selectors[key].addEventListener('change', () => {
      const desired = { ...variantChoice(variant), [key]: selectors[key].value };
      const candidate = story.variants.find(item => dimensions.every(other => variantChoice(item)[other] === desired[other]));
      if (!candidate) {
        renderSelectors();
        feedback.textContent = 'This combination has not been captured. Choose one of the available variants.';
        return;
      }
      variant = candidate;
      render();
      updateUrl();
    }));
    zoom.addEventListener('change', () => { resizeImage(); updateUrl(); });
    search.addEventListener('input', filterStories);
    family.addEventListener('change', filterStories);
    element<HTMLButtonElement>('[data-lab-reset]').addEventListener('click', () => { search.value = ''; family.value = 'all'; filterStories(); search.focus(); });
    search.addEventListener('keydown', event => { if (event.key === 'Escape') { search.value = ''; filterStories(); } });
    document.addEventListener('keydown', event => {
      if (event.key !== '/' || event.ctrlKey || event.metaKey || event.altKey || event.target instanceof Element && event.target.closest('input, textarea, select, [contenteditable="true"]')) return;
      event.preventDefault();
      search.focus();
    });
    share.disabled = false;
    share.addEventListener('click', async () => {
      updateUrl();
      try { await navigator.clipboard.writeText(window.location.href); feedback.textContent = 'Story link copied, including palette, state, density, viewport and zoom.'; }
      catch { feedback.textContent = 'Copy the current address from your browser to share this story.'; }
    });
    new ResizeObserver(resizeImage).observe(stage);
    window.addEventListener('popstate', readUrl);
    resizeImage();
    readUrl();
  } catch (error) {
    feedback.textContent = error instanceof Error ? error.message : 'The component explorer could not start. Regenerate its native capture manifest.';
  }
}
if (typeof document !== 'undefined') {
  document.querySelectorAll<HTMLElement>('[data-component-lab]').forEach(initializeLab);
}
