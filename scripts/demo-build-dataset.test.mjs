import { test } from 'node:test';
import assert from 'node:assert/strict';
import { checkDataset } from '../marketing/demo/build-dataset.mjs';

test('production rejects an absent real history dataset', () => {
  assert.throws(() => checkDataset('missing-demo-dataset'), /dataset is required/);
});
test('an incomplete exported candidate cannot pass the production gate', { skip: !process.env.DEMO_DATASET_PATH }, () => {
  assert.throws(() => checkDataset(process.env.DEMO_DATASET_PATH), /missing references/);
  assert.doesNotThrow(() => checkDataset(process.env.DEMO_DATASET_PATH, true));
});
