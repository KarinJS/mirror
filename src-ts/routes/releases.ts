import { getWhitelists } from '../config.ts';
import { notFound } from '../http.ts';
import { proxyUpstream } from '../proxy.ts';

/** /gh/<owner>/<repo>/releases/download/<tag>/<file> */
export async function handleReleases(url: URL, req: Request): Promise<Response | null> {
  const segs = url.pathname.split('/').filter(Boolean);
  // ['gh', owner, repo, 'releases', 'download', tag, file]
  if (segs.length !== 7) return null;
  if (segs[0] !== 'gh' || segs[3] !== 'releases' || segs[4] !== 'download') return null;
  const [, owner, repo, , , tag, file] = segs as [string, string, string, string, string, string, string];

  const wl = getWhitelists().releases;
  const files = wl[owner]?.[repo];
  if (!files || !files.includes(file)) return notFound();

  const target = `https://github.com/${enc(owner)}/${enc(repo)}/releases/download/${enc(tag)}/${enc(file)}`;
  return proxyUpstream(req, target, { ttl: -1 });
}

function enc(s: string): string {
  return encodeURIComponent(s);
}
