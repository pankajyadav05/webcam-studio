export function encodeFrame(id, jpeg, quality = 'balanced', epoch = 0) {
  const body = new Uint8Array(16 + jpeg.byteLength);
  body.set([87, 83, 73, 49]);
  const view = new DataView(body.buffer);
  view.setUint32(4, id, true);
  view.setUint32(12, epoch, true);
  view.setUint32(8, quality === 'accurate' ? 1 : 0, true);
  body.set(new Uint8Array(jpeg), 16);
  return body;
}
export function decodeMask(buffer, expectedId) {
  if (!(buffer instanceof ArrayBuffer)) buffer = new Uint8Array(buffer).buffer;
  if (buffer.byteLength < 20) throw new Error('Truncated native mask');
  const view = new DataView(buffer);
  if (view.getUint32(0, true) !== 0x314d5357) throw new Error('Invalid native mask version');
  const id = view.getUint32(4, true), width = view.getUint32(8, true), height = view.getUint32(12, true);
  if (id !== expectedId || !width || !height || width * height > 16777216 || buffer.byteLength !== 20 + width * height) {
    throw new Error('Invalid native mask identity or shape');
  }
  return { id, width, height, segmentMs: view.getFloat32(16, true), alpha: new Uint8Array(buffer, 20) };
}
