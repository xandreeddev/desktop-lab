import test from 'node:test';
import assert from 'node:assert/strict';
import { chooseVariant, matchesStory, variantChoice } from '../src/scripts/component-lab.ts';

const variants = [];
for (const palette of ['dark', 'forest']) {
  for (const state of ['default', 'focus']) {
    for (const scale of [1, 2]) {
      for (const viewport of ['regular', 'narrow']) {
        variants.push(Object.freeze({
          id: `${palette}-${state}-${scale}-${viewport}`,
          palette, state, density: { numerator: scale, denominator: 1 },
          viewport: { id: viewport, width: viewport === 'regular' ? 800 : 352, height: 512 },
          image: { path: 'images/fixture.png', width: 800 * scale, height: 512 * scale, sha256: '' },
        }));
      }
    }
  }
}
const story = Object.freeze({
  id: 'button', family: 'Controls', title: 'Action buttons',
  description: 'Keyboard and pointer states', tokens: ['component.button.radius'],
  variants: Object.freeze(variants),
});

test('exact shared-link choices select the captured variant without changing density or state', () => {
  const requested = { palette: 'forest', state: 'focus', scale: '2', viewport: 'narrow' };
  const actual = chooseVariant(story, requested);
  assert.deepEqual(variantChoice(actual), requested);
  assert.ok(story.variants.includes(actual));
});

test('switching to a story without the previous state preserves the other real choices', () => {
  const actual = chooseVariant(story, { palette: 'forest', state: 'not-a-state', scale: '2', viewport: 'narrow' });
  assert.deepEqual(variantChoice(actual), { palette: 'forest', state: 'default', scale: '2', viewport: 'narrow' });
});

test('invalid URL choices fall back to a real captured combination deterministically', () => {
  assert.equal(chooseVariant(story, { palette: 'missing', scale: 'Infinity' }), story.variants[0]);
  assert.equal(chooseVariant({ ...story, variants: [] }, {}), undefined);
});

test('search matches all words across source-backed title, description and tokens', () => {
  assert.equal(matchesStory(story, 'BUTTON keyboard', 'Controls'), true);
  assert.equal(matchesStory(story, 'component.button.radius', 'all'), true);
  assert.equal(matchesStory(story, 'keyboard network', 'all'), false);
  assert.equal(matchesStory(story, '', 'Forms'), false);
  assert.equal(matchesStory(story, '   ', 'all'), true);
});
