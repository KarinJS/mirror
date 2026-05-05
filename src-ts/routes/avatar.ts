import { getConfig, getWhitelists } from '../config.ts';
import { notFound } from '../http.ts';
import { proxyUpstream } from '../proxy.ts';

/** /avatar/<user>.png */
export async function handleAvatar(url: URL, req: Request): Promise<Response | null> {
  const segs = url.pathname.split('/').filter(Boolean);
  if (segs.length !== 2 || segs[0] !== 'avatar') return null;
  let user = segs[1]!;
  if (!user.toLowerCase().endsWith('.png')) return notFound();
  user = user.slice(0, -4);
  if (!user) return notFound();

  const wl = getWhitelists().avatar;
  if (!wl.includes(user)) return notFound();

  return proxyUpstream(req, `https://github.com/${encodeURIComponent(user)}.png`, { ttl: getConfig().cacheTTL.avatar });
}
