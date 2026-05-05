export const TEXT_404 = 'Not Found';
export const TEXT_405 = 'Method Not Allowed';
export const TEXT_413 = 'Payload Too Large';
export const TEXT_451 = 'Unavailable For Legal Reasons';
export const TEXT_502 = 'Bad Gateway';
export const TEXT_504 = 'Gateway Timeout';

export function plain(
  status: number,
  body: string,
  extra?: Record<string, string>,
): Response {
  return new Response(body, {
    status,
    headers: { 'content-type': 'text/plain; charset=utf-8', ...(extra ?? {}) },
  });
}

export function notFound(): Response {
  return plain(404, TEXT_404);
}

export function methodNotAllowed(): Response {
  return plain(405, TEXT_405, { allow: 'GET, HEAD' });
}

export function geoBlocked(): Response {
  return plain(451, TEXT_451);
}

/** RFC 7231 §6.4.7 — 308 preserves method, removes query. */
export function redirectStripQuery(pathname: string): Response {
  return new Response(null, {
    status: 308,
    headers: { location: pathname || '/' },
  });
}

export function redirectCanonical(pathname: string, ttl: number): Response {
  return new Response(null, {
    status: 302,
    headers: {
      location: pathname || '/',
      'cache-control': ttl <= 0 ? 'no-store' : `public, max-age=${Math.floor(ttl)}`,
    },
  });
}

export function withCors(res: Response): Response {
  const h = new Headers(res.headers);
  h.set('access-control-allow-origin', '*');
  h.set('access-control-allow-methods', 'GET, HEAD, OPTIONS');
  h.set('vary', appendVary(h.get('vary'), 'Origin'));
  return new Response(res.body, { status: res.status, headers: h });
}

function appendVary(existing: string | null, name: string): string {
  if (!existing) return name;
  const parts = existing.split(',').map((s) => s.trim());
  if (parts.includes(name)) return existing;
  parts.push(name);
  return parts.join(', ');
}

export function clientIp(req: Request, trust: boolean): string {
  if (trust) {
    const xff = req.headers.get('x-forwarded-for');
    if (xff) return xff.split(',')[0]!.trim();
    const real = req.headers.get('x-real-ip');
    if (real) return real;
  }
  return '';
}

export function clientCountry(req: Request, headerName: string): string {
  return (req.headers.get(headerName) ?? '').trim().toUpperCase();
}
