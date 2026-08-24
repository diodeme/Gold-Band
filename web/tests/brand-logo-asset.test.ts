import { readFileSync, statSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { inflateSync } from 'node:zlib';

import { describe, expect, it } from 'vitest';

const webLogoPath = fileURLToPath(new URL('../public/logo.svg', import.meta.url));
const webLogo = readFileSync(webLogoPath, 'utf8');
const tauriLogoSource = readFileSync(
  fileURLToPath(new URL('../../src-tauri/icons/logo-source.svg', import.meta.url)),
  'utf8',
);
const windowsTaskbarIconPath = fileURLToPath(new URL('../../src-tauri/icons/32x32.png', import.meta.url));
const windowsIcoPath = fileURLToPath(new URL('../../src-tauri/icons/icon.ico', import.meta.url));

function vectorPaths(svg: string) {
  return Array.from(svg.matchAll(/<path\b[^>]*\bd="([^"]+)"/g), match => match[1]);
}

function decodeRgbaPng(source: Buffer) {
  const signatureLength = 8;
  let offset = signatureLength;
  let width = 0;
  let height = 0;
  const idatChunks: Buffer[] = [];

  while (offset < source.length) {
    const length = source.readUInt32BE(offset);
    const type = source.toString('ascii', offset + 4, offset + 8);
    const dataStart = offset + 8;
    const dataEnd = dataStart + length;
    if (type === 'IHDR') {
      width = source.readUInt32BE(dataStart);
      height = source.readUInt32BE(dataStart + 4);
      if (source[dataStart + 8] !== 8 || source[dataStart + 9] !== 6) {
        throw new Error('Expected an 8-bit RGBA PNG icon.');
      }
    } else if (type === 'IDAT') {
      idatChunks.push(source.subarray(dataStart, dataEnd));
    }
    offset = dataEnd + 4;
  }

  const stride = width * 4;
  const encoded = inflateSync(Buffer.concat(idatChunks));
  const pixels = Buffer.alloc(stride * height);
  const paeth = (left: number, up: number, upLeft: number) => {
    const estimate = left + up - upLeft;
    const leftDistance = Math.abs(estimate - left);
    const upDistance = Math.abs(estimate - up);
    const upLeftDistance = Math.abs(estimate - upLeft);
    return leftDistance <= upDistance && leftDistance <= upLeftDistance ? left : upDistance <= upLeftDistance ? up : upLeft;
  };

  for (let row = 0; row < height; row += 1) {
    const filter = encoded[row * (stride + 1)];
    const rowStart = row * (stride + 1) + 1;
    const pixelStart = row * stride;
    for (let column = 0; column < stride; column += 1) {
      const left = column >= 4 ? pixels[pixelStart + column - 4] : 0;
      const up = row > 0 ? pixels[pixelStart - stride + column] : 0;
      const upLeft = row > 0 && column >= 4 ? pixels[pixelStart - stride + column - 4] : 0;
      const value = encoded[rowStart + column];
      const predictor = filter === 0 ? 0 : filter === 1 ? left : filter === 2 ? up : filter === 3 ? Math.floor((left + up) / 2) : paeth(left, up, upLeft);
      pixels[pixelStart + column] = (value + predictor) & 0xff;
    }
  }
  return { width, height, pixels };
}

function decodeRgbaPngFile(path: string) {
  return decodeRgbaPng(readFileSync(path));
}

function readIcoFrames(source: Buffer) {
  const frameCount = source.readUInt16LE(4);
  return Array.from({ length: frameCount }, (_, index) => {
    const entryOffset = 6 + index * 16;
    return {
      width: source[entryOffset] || 256,
      height: source[entryOffset + 1] || 256,
      bitsPerPixel: source.readUInt16LE(entryOffset + 6),
      length: source.readUInt32LE(entryOffset + 8),
      offset: source.readUInt32LE(entryOffset + 12),
    };
  });
}

describe('brand logo asset', () => {
  it('keeps the frontend logo as a compact true vector asset', () => {
    expect(webLogo).toContain('viewBox="0 0 1254 1254"');
    expect(vectorPaths(webLogo)).toHaveLength(6);
    expect(webLogo).not.toContain('<image');
    expect(webLogo).not.toContain('data:image/');
    expect(statSync(webLogoPath).size).toBeLessThan(128 * 1024);
  });

  it('derives the square Tauri icon source from the same vector paths', () => {
    expect(tauriLogoSource).toContain('viewBox="0 0 2048 2048"');
    expect(tauriLogoSource).not.toContain('<image');
    expect(vectorPaths(tauriLogoSource)).toEqual(vectorPaths(webLogo));
  });

  it('keeps the Tauri Windows default window icon at 32px with complete DPI frames', () => {
    const frames = readIcoFrames(readFileSync(windowsIcoPath));

    // Tauri decodes the first ICO frame for the live window icon. Keep 32px first
    // so Windows does not upscale a 16px frame on common taskbar DPI settings.
    expect(frames.map(({ width, height }) => [width, height])).toEqual([
      [32, 32],
      [16, 16],
      [24, 24],
      [48, 48],
      [64, 64],
      [256, 256],
    ]);
    expect(frames.every(frame => frame.bitsPerPixel === 32)).toBe(true);
  });

  it('keeps the Windows taskbar icon free from white matte pixels', () => {
    const { width, height, pixels } = decodeRgbaPngFile(windowsTaskbarIconPath);
    const whiteMattePixels: number[] = [];
    for (let offset = 0; offset < pixels.length; offset += 4) {
      const [red, green, blue, alpha] = pixels.subarray(offset, offset + 4);
      if (alpha > 0 && alpha < 255 && red >= 245 && green >= 245 && blue >= 245) {
        whiteMattePixels.push(offset / 4);
      }
    }

    expect([width, height]).toEqual([32, 32]);
    expect(whiteMattePixels).toEqual([]);

    const ico = readFileSync(windowsIcoPath);
    for (const { offset, length } of readIcoFrames(ico)) {
      const frame = decodeRgbaPng(ico.subarray(offset, offset + length));
      for (let offset = 0; offset < frame.pixels.length; offset += 4) {
        const [red, green, blue, alpha] = frame.pixels.subarray(offset, offset + 4);
        expect(alpha > 0 && alpha < 255 && red >= 245 && green >= 245 && blue >= 245).toBe(false);
      }
    }
  });

  it('keeps the README header on the canonical source asset', () => {
    for (const relative of ['../../README.md', '../../README.zh-CN.md']) {
      const source = readFileSync(fileURLToPath(new URL(relative, import.meta.url)), 'utf8');
      expect(source).toContain('<img src="web/public/logo.svg"');
      expect(source).not.toContain('src-tauri/icons/icon.png');
    }
  });

  it('keeps brand consumers on the canonical frontend logo path', () => {
    const consumers = [
      '../index.html',
      '../src/components/AppTitleBar.tsx',
      '../src/components/BrandLoadingState.tsx',
      '../src/lib/agent-icons.ts',
      '../src/pages/WorkspaceSelectPage.tsx',
    ].map(relative => readFileSync(fileURLToPath(new URL(relative, import.meta.url)), 'utf8'));

    for (const source of consumers) {
      expect(source).toContain('/logo.svg');
    }
  });
});
