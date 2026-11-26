/// <reference lib="webworker" />
// Runs the WASM engine off the main thread so the UI never freezes.
import init, { vectorize } from './pkg/openvectorizer_wasm.js';
import type { Options } from './generated/Options';
import type { Output } from './generated/Output';

export type WorkerRequest = {
  id: number;
  rgba: Uint8ClampedArray;
  width: number;
  height: number;
  options: Partial<Options>;
};

export type WorkerResponse =
  { id: number; ok: true; output: Output; ms: number } | { id: number; ok: false; error: string };

const ready = init();

self.onmessage = async (event: MessageEvent<WorkerRequest>) => {
  const { id, rgba, width, height, options } = event.data;
  try {
    await ready;
    const t0 = performance.now();
    const output = vectorize(new Uint8Array(rgba.buffer), width, height, options) as Output;
    const response: WorkerResponse = { id, ok: true, output, ms: performance.now() - t0 };
    self.postMessage(response);
  } catch (e) {
    const response: WorkerResponse = { id, ok: false, error: e instanceof Error ? e.message : String(e) };
    self.postMessage(response);
  }
};
