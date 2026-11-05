// Module hook for test-history.mjs: when the console page
// script imports the WASM package, give it the Node stub
// (real WASM loaded from disk) instead of the browser build,
// whose init() would try to fetch the .wasm over HTTP.
// Only the page script's import is redirected; every other
// specifier resolves normally.
export async function resolve(specifier, context, nextResolve) {
  if (
    specifier === '../pkg/niva_playground.js' &&
    context.parentURL &&
    context.parentURL.includes('/console/console.js')
  ) {
    return {
      url: new URL('./test-history-wasm-stub.mjs', import.meta.url).href,
      shortCircuit: true,
    };
  }
  return nextResolve(specifier, context);
}
