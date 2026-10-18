// Which OS the webview runs on (D16): WebView2's user agent always contains "Windows NT".
export type Platform = 'macos' | 'windows';

export function detectPlatform(userAgent: string): Platform {
  return /Windows/.test(userAgent) ? 'windows' : 'macos';
}

export const platform: Platform = detectPlatform(typeof navigator === 'undefined' ? '' : navigator.userAgent);
export const isWindows = platform === 'windows';

/** `<html data-platform="windows|macos">`, for platform-specific CSS. */
export function applyPlatformAttribute(doc: Document = document): void {
  doc.documentElement.dataset.platform = platform;
}
