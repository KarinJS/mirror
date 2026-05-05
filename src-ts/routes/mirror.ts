import { getConfig, getWhitelists } from '../config.ts';
import { plain } from '../http.ts';
import { proxyUpstream } from '../proxy.ts';
import type { MirrorRule } from '../types.ts';

/** /mirror/<host>/<path...> */
export async function handleMirror(url: URL, req: Request): Promise<Response | null> {
  const segs = url.pathname.split('/').filter(Boolean);
  if (segs.length < 3 || segs[0] !== 'mirror') return null;
  const host = segs[1]!;
  const path = segs.slice(2).map(encodeURIComponent).join('/');
  const target = `https://${host}/${path}`;

  const wl = getWhitelists().mirror;
  const rule = wl[target];
  if (rule === undefined) return plain(404, 'Not Found');

  const { ttl, maxSize } = normalizeRule(rule);
  const cfg = getConfig().mirror;
  return proxyUpstream(req, target, { ttl, maxSize: maxSize ?? cfg.defaultMaxSize });
}

function normalizeRule(rule: MirrorRule): { ttl: number; maxSize?: number } {
  if (typeof rule === 'number') return { ttl: rule };
  return { ttl: rule.ttl, maxSize: rule.maxSize };
}
