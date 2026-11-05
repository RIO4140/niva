// WASM stub for test-history.mjs: the console page calls
// init() with no arguments (browser fetch); here the real
// WASM bytes are loaded from disk, exactly like the other
// Node tests do.
import { default as realInit, NivaConsole } from './pkg/niva_playground.js';
import fs from 'node:fs';

let inited = false;
export default async function init() {
  if (inited) return;
  await realInit(fs.readFileSync(new URL('./pkg/niva_playground_bg.wasm', import.meta.url)));
  inited = true;
}
export { NivaConsole };
