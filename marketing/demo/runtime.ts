import { createDemoApi } from './api';
import { createArchiveReader } from './archive';

const archiveUrl = import.meta.env.VITE_DEMO_ARCHIVE_URL as string | undefined;
const reader = archiveUrl ? createArchiveReader(archiveUrl) : null;
let catalogRequest: ReturnType<NonNullable<typeof reader>['catalog']> | null = null;
export const demoArchive = reader ? { reader, catalog: () => {
  catalogRequest ??= reader.catalog().catch(error => { catalogRequest = null; throw error; });
  return catalogRequest;
} } : undefined;

let storage: Storage | undefined;
try { storage = window.localStorage; } catch { /* Browser storage is optional. */ }
export const browserApi = createDemoApi(storage, demoArchive);
export const desktopApi = browserApi;
