import type { GeoConfig } from './types.ts';

export type GeoDecision = 'allow' | 'deny';

export function checkGeo(geo: GeoConfig, headers: Headers): GeoDecision {
  if (geo.mode === 'off') return 'allow';
  const raw = headers.get(geo.headerName);
  const country = (raw ?? '').trim().toUpperCase();
  const list = geo.countries.map((c) => c.toUpperCase());
  if (geo.mode === 'allow') {
    if (!country) return 'deny';
    return list.includes(country) ? 'allow' : 'deny';
  }
  // deny mode
  if (!country) return 'allow';
  return list.includes(country) ? 'deny' : 'allow';
}
