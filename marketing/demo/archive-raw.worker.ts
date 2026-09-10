import { queryArchiveRawFrames } from './archive-raw-query';

self.onmessage = async ({ data }) => {
  try {
    self.postMessage({ result: await queryArchiveRawFrames(data.url, data.index, data.query) });
  } catch (error) {
    self.postMessage({ error: { code: typeof error === 'object' && error && 'code' in error
      ? error.code : 'demo.resource-not-found', params: {} } });
  }
};
