const test = require('node:test');
const assert = require('node:assert');
const { slugify } = require('../src/slug');

test('slugify lowercases a single word', () => {
  assert.strictEqual(slugify('Hello'), 'hello');
});
