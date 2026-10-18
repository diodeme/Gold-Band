import { createDragDropManager } from 'dnd-core';
import { HTML5Backend } from 'react-dnd-html5-backend';

/**
 * The single drag-and-drop manager for every react-arborist tree in the app.
 *
 * Without it each Tree creates react-dnd's ref-counted global manager, which
 * StrictMode replays and layout remounts can reset while rows are still
 * registered, starting a second HTML5 backend on `window`.
 */
export const workspaceTreeDndManager = createDragDropManager(HTML5Backend);
