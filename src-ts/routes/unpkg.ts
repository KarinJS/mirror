import { getConfig, getWhitelists } from '../config.ts';
import { notFound, redirectCanonical } from '../http.ts';
import { proxyUpstream, resolveUpstreamUrl } from '../proxy.ts';

interface ParsedUnpkgPath {
  pkgWithVersion: string;
  pkgName: string;
  fileSegs: string[];
}

/**
 * /unpkg/<pkg>[@version]/<file...>
 * Supports scoped packages: /unpkg/@scope/name@1.2.3/dist/x.js
 * Whitelist key is the package name without version (e.g. "karin" or "@scope/name").
 */
export async function handleUnpkg(url: URL, req: Request): Promise<Response | null> {
  const requested = parseMirrorPath(url.pathname);
  if (!requested) return null;

  const target = toUnpkgUrl(requested, url.pathname.endsWith('/'));
  const resolved = await resolveUpstreamUrl(req, target);
  if (resolved instanceof Response) return resolved;

  const finalUrl = new URL(resolved);
  if (finalUrl.hostname !== 'unpkg.com') return notFound();

  const canonical = parseUpstreamPath(finalUrl.pathname);
  if (!canonical || canonical.fileSegs.length === 0) return notFound();

  const file = canonical.fileSegs.join('/');
  const wl = getWhitelists().unpkg;
  const allowed = wl[canonical.pkgName];
  if (!allowed || !allowed.includes(file)) return notFound();

  const canonicalPath = `/unpkg${finalUrl.pathname}`;
  const ttl = getConfig().cacheTTL.unpkg;
  if (canonicalPath !== url.pathname) {
    return redirectCanonical(canonicalPath, ttl);
  }

  return proxyUpstream(req, finalUrl.toString(), { ttl });
}

function parseMirrorPath(pathname: string): ParsedUnpkgPath | null {
  const segs = pathname.split('/').filter(Boolean);
  if (segs.length < 2 || segs[0] !== 'unpkg') return null;
  return parsePackageSegments(segs, 1);
}

function parseUpstreamPath(pathname: string): ParsedUnpkgPath | null {
  const segs = pathname.split('/').filter(Boolean);
  return parsePackageSegments(segs, 0);
}

function parsePackageSegments(segs: string[], start: number): ParsedUnpkgPath | null {
  if (start >= segs.length) return null;

  let pkgEnd: number;
  let pkgWithVersion: string;
  if (segs[start]!.startsWith('@')) {
    if (segs.length <= start + 1) return null;
    pkgWithVersion = `${segs[start]}/${segs[start + 1]}`;
    pkgEnd = start + 2;
  } else {
    pkgWithVersion = segs[start]!;
    pkgEnd = start + 1;
  }

  const at = pkgWithVersion.lastIndexOf('@');
  const pkgName = at > 0 ? pkgWithVersion.slice(0, at) : pkgWithVersion;

  return {
    pkgWithVersion,
    pkgName,
    fileSegs: segs.slice(pkgEnd),
  };
}

function toUnpkgUrl(parsed: ParsedUnpkgPath, trailingSlash: boolean): string {
  const parts = parsed.fileSegs.map(encodeURIComponent).join('/');
  const suffix = parts ? `/${parts}` : trailingSlash ? '/' : '';
  return `https://unpkg.com/${parsed.pkgWithVersion}${suffix}`;
}
