import { describe, expect, test } from 'vitest';
import { detectPlatform } from './platform';

describe('detectPlatform', () => {
  test('WebView2 user agents are Windows', () => {
    expect(detectPlatform('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0')).toBe('windows');
  });
  test('WKWebView user agents are macOS', () => {
    expect(detectPlatform('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)')).toBe('macos');
  });
});
