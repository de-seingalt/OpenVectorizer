// Single entry point to the vectorization engine. UI code must only use this module.
import type { Options } from './generated/Options';
import type { Output } from './generated/Output';
import type { WorkerRequest, WorkerResponse } from './worker';

export type { CornerEdit } from './generated/CornerEdit';
export type { CornerInfo } from './generated/CornerInfo';
export type { CornerMode } from './generated/CornerMode';
export type { Options } from './generated/Options';
export type { Palette } from './generated/Palette';
export type { PaletteEdit } from './generated/PaletteEdit';
export type { Output } from './generated/Output';
export type { Preset } from './generated/Preset';
export type { Stacking } from './generated/Stacking';
export type { Stats } from './generated/Stats';

/** Must mirror `Options::default()` in crates/core/src/options.rs. */
export const DEFAULT_OPTIONS: Options = {
  preset: 'logo',
  detail: 0.6,
  smoothness: 0.4,
  minAreaPx: 4,
  stacking: 'stacked',
  maxColors: 0,
  paletteEdit: null,
  corners: { mode: 'automatic', sharpCount: null, radius: 0, edits: [] },
  shapes: true,
};

export type Result = { output: Output; ms: number };

/**
 * Runs vectorizations in a Web Worker. Only the most recent request matters:
 * while one job runs, newer requests replace the queued one, and superseded
 * promises reject with `SupersededError` (callers should ignore it).
 */
export class Engine {
  #worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
  #nextId = 1;
  #busy = false;
  #queued: { req: WorkerRequest; resolve: (r: Result) => void; reject: (e: Error) => void } | null = null;
  #current: { id: number; resolve: (r: Result) => void; reject: (e: Error) => void } | null = null;

  constructor() {
    this.#worker.onmessage = (event: MessageEvent<WorkerResponse>) => {
      const res = event.data;
      const current = this.#current;
      this.#current = null;
      this.#busy = false;
      if (current && current.id === res.id) {
        if (this.#queued) current.reject(new SupersededError());
        else if (res.ok) current.resolve({ output: res.output, ms: res.ms });
        else current.reject(new Error(res.error));
      }
      this.#pump();
    };
  }

  vectorize(image: ImageData, options: Options): Promise<Result> {
    return new Promise((resolve, reject) => {
      this.#queued?.reject(new SupersededError());
      // Copy: the buffer is transferred to the worker.
      const rgba = new Uint8ClampedArray(image.data);
      this.#queued = {
        req: { id: this.#nextId++, rgba, width: image.width, height: image.height, options: { ...options } },
        resolve,
        reject,
      };
      this.#pump();
    });
  }

  #pump() {
    if (this.#busy || !this.#queued) return;
    const { req, resolve, reject } = this.#queued;
    this.#queued = null;
    this.#busy = true;
    this.#current = { id: req.id, resolve, reject };
    this.#worker.postMessage(req, [req.rgba.buffer]);
  }
}

export class SupersededError extends Error {
  constructor() {
    super('superseded by a newer request');
    this.name = 'SupersededError';
  }
}
