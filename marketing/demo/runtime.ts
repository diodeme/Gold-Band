import { createDemoApi } from './api';

let storage: Storage | undefined;
try { storage = window.localStorage; } catch { /* Browser storage is optional. */ }
export const browserApi = createDemoApi(storage);
export const desktopApi = browserApi;
