import { getConfig, getWhitelists } from '../config.ts';
import { notFound } from '../http.ts';
import { proxyUpstream } from '../proxy.ts';

/** /raw/<owner>/<repo>/<branch>/<path...> */
export async function handleRaw(url: URL, req: Request): Promise<Response | null> {
  const segs = url.pathname.split('/').filter(Boolean);
  if (segs.length < 5 || segs[0] !== 'raw') return null;
  const owner = segs[1]!;
  const repo = segs[2]!;
  const branch = segs[3]!;
  const file = segs.slice(4).join('/');

  const wl = getWhitelists().raw;
  const entries = wl[owner]?.[repo];
  if (!entries) return notFound();

  // HEAD in whitelist = wildcard for any branch. Otherwise branch must match exactly.
  const matched = entries.some(
    (e) => e.file === file && (e.branch === 'HEAD' || e.branch === branch),
  );
  if (!matched) return notFound();

  const path = file.split('/').map(encodeURIComponent).join('/');
  const target = `https://raw.githubusercontent.com/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/${encodeURIComponent(branch)}/${path}`;
  return proxyUpstream(req, target, { ttl: getConfig().cacheTTL.raw });
}
