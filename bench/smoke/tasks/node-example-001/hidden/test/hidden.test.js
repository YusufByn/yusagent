const test = require('node:test');
const assert = require('node:assert');
const { slugify } = require('../src/slug');

test('slugify collapses separators', () => {
  assert.strictEqual(slugify('Hello,  World'), 'hello-world');
});

test('slugify trims separators at both ends', () => {
  assert.strictEqual(slugify('  Hi there! '), 'hi-there');
});

test('slugify keeps digits', () => {
  assert.strictEqual(slugify('route66'), 'route66');
});
