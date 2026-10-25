// Worker CSP is not inherited from a document's meta tag. Keep the model
// loader offline even when a vendor task tries to flush usage telemetry.
export function offlineFetch(baseURL, fetchImpl = globalThis.fetch) {
  const base = new URL(baseURL);
  return (input, options) => {
    const url = new URL(typeof input === 'object' && input?.url ? input.url : input, base);
    if (url.protocol !== base.protocol || url.host !== base.host) {
      return Promise.reject(new TypeError('The offline person worker blocks external requests.'));
    }
    return fetchImpl.call(globalThis, input, options);
  };
}
