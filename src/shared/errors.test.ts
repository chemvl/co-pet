import { describe, expect, it } from 'vitest';
import { errorMessage, errorText } from './errors';
import { translate, type TFunction } from './i18n';

const t: TFunction = (key, params) => translate('vi', key, params);

describe('errorMessage', () => {
  it('translates a known error code', () => {
    expect(errorMessage(t, 'duplicate')).toBe('Đã có danh mục trùng tên.');
    expect(errorMessage(t, new Error('notFound'))).toBe('Mục này không còn nữa.');
  });

  it('returns the raw text for anything else', () => {
    expect(errorMessage(t, 'disk full')).toBe('disk full');
    expect(errorText(42)).toBe('42');
  });
});
