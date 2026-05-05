import { describe, expect, test } from 'bun:test';
import { checkGeo } from '../src/geo.ts';

describe('geo', () => {
  test('off mode always allows', () => {
    expect(checkGeo({ mode: 'off', headerName: 'X-C', countries: [] }, new Headers())).toBe('allow');
  });
  test('allow list — country in list', () => {
    expect(
      checkGeo(
        { mode: 'allow', headerName: 'X-C', countries: ['CN', 'HK'] },
        new Headers({ 'X-C': 'cn' }),
      ),
    ).toBe('allow');
  });
  test('allow list — country missing', () => {
    expect(
      checkGeo(
        { mode: 'allow', headerName: 'X-C', countries: ['CN'] },
        new Headers({ 'X-C': 'us' }),
      ),
    ).toBe('deny');
  });
  test('allow list — header missing denies', () => {
    expect(
      checkGeo({ mode: 'allow', headerName: 'X-C', countries: ['CN'] }, new Headers()),
    ).toBe('deny');
  });
  test('deny list — country listed denies', () => {
    expect(
      checkGeo(
        { mode: 'deny', headerName: 'X-C', countries: ['RU'] },
        new Headers({ 'X-C': 'ru' }),
      ),
    ).toBe('deny');
  });
  test('deny list — header missing allows', () => {
    expect(
      checkGeo({ mode: 'deny', headerName: 'X-C', countries: ['RU'] }, new Headers()),
    ).toBe('allow');
  });
});
