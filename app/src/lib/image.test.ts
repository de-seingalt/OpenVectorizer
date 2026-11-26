import { describe, expect, it } from 'vitest';
import { baseName, fitWithin } from './image';

describe('fitWithin', () => {
  it('keeps small images unchanged', () => {
    expect(fitWithin(800, 600, 1_000_000)).toEqual([800, 600]);
  });
  it('downscales large images keeping aspect ratio', () => {
    const [w, h] = fitWithin(4000, 2000, 2_000_000);
    expect(w * h).toBeLessThanOrEqual(2_000_000);
    expect(w / h).toBeCloseTo(2, 2);
  });
});

describe('baseName', () => {
  it('strips the extension', () => {
    expect(baseName('logo.final.png')).toBe('logo.final');
    expect(baseName('README')).toBe('README');
  });
});
