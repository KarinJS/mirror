import { getConfig } from './config.ts';
import { plain, TEXT_413, TEXT_502, TEXT_504 } from './http.ts';
import { log } from './log.ts';

const FORWARD_REQ_HEADERS = ['range', 'if-none-match', 'if-modified-since', 'accept', 'user-agent'];
const FORWARD_RES_HEADERS = [
  'content-type',
  'content-range',
  'accept-ranges',
  'last-modified',
  'etag',
  'content-disposition',
];

export interface ProxyOptions {
  ttl: number;
  maxSize?: number;
}

export async function proxyUpstream(req: Request, target: string, options: ProxyOptions): Promise<Response> {
  const cfg = getConfig().mirror;
  const limit = Math.min(options.maxSize ?? cfg.absoluteMaxSize, cfg.absoluteMaxSize);

  let upstream: Response;
  try {
    upstream = await fetchUpstream(req, target, { method: req.method, redirect: 'follow' });
  } catch (err) {
    return upstreamError(target, err);
  }

  const cl = upstream.headers.get('content-length');
  if (cl && Number(cl) > limit) {
    return plain(413, TEXT_413);
  }

  const outHeaders = new Headers();
  for (const name of FORWARD_RES_HEADERS) {
    const v = upstream.headers.get(name);
    if (v) outHeaders.set(name, v);
  }
  applyTTL(outHeaders, options.ttl, upstream);

  if (req.method === 'HEAD' || upstream.body === null) {
    return new Response(null, { status: upstream.status, headers: outHeaders });
  }

  const limited = limitBody(upstream.body, limit);
  return new Response(limited, { status: upstream.status, headers: outHeaders });
}

export async function resolveUpstreamUrl(req: Request, target: string): Promise<string | Response> {
  let current = target;

  for (let i = 0; i < 10; i++) {
    let upstream: Response;
    try {
      upstream = await fetchUpstream(req, current, { method: 'HEAD', redirect: 'manual' });
    } catch (err) {
      return upstreamError(current, err);
    }

    try {
      if (upstream.status === 405) {
        try {
          upstream = await fetchUpstream(req, current, { method: 'GET', redirect: 'manual' });
        } catch (err) {
          return upstreamError(current, err);
        }
      }

      const location = upstream.headers.get('location');
      if (location && isRedirectStatus(upstream.status)) {
        current = new URL(location, current).toString();
        continue;
      }

      return current;
    } finally {
      await cancelBody(upstream.body);
    }
  }

  log.warn('upstream redirect loop', { target });
  return plain(502, TEXT_502);
}

export function applyTTL(headers: Headers, ttl: number, upstream: Response): void {
  if (ttl === -2) {
    const cc = upstream.headers.get('cache-control');
    if (cc) headers.set('cache-control', cc);
    const etag = upstream.headers.get('etag');
    if (etag) headers.set('etag', etag);
    return;
  }
  if (ttl === -1) {
    headers.set('cache-control', 'public, max-age=31536000, immutable');
    return;
  }
  if (ttl === 0) {
    headers.set('cache-control', 'no-store');
    headers.delete('etag');
    headers.delete('last-modified');
    return;
  }
  headers.set('cache-control', `public, max-age=${Math.floor(ttl)}`);
}

async function fetchUpstream(
  req: Request,
  target: string,
  init: { method: string; redirect: 'follow' | 'manual' },
): Promise<Response> {
  const cfg = getConfig().mirror;
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), cfg.fetchTimeoutMs);

  try {
    return await fetch(target, {
      method: init.method,
      headers: buildForwardHeaders(req),
      redirect: init.redirect,
      signal: ctrl.signal,
    });
  } finally {
    clearTimeout(timer);
  }
}

function buildForwardHeaders(req: Request): Headers {
  const fwdHeaders = new Headers();
  for (const name of FORWARD_REQ_HEADERS) {
    const v = req.headers.get(name);
    if (v) fwdHeaders.set(name, v);
  }
  fwdHeaders.set('accept-encoding', 'identity');
  return fwdHeaders;
}

function upstreamError(target: string, err: unknown): Response {
  const aborted = (err as { name?: string }).name === 'AbortError';
  log.warn('upstream fetch failed', { target, err: String(err) });
  return plain(aborted ? 504 : 502, aborted ? TEXT_504 : TEXT_502);
}

function isRedirectStatus(status: number): boolean {
  return status === 301 || status === 302 || status === 303 || status === 307 || status === 308;
}

async function cancelBody(body: ReadableStream<Uint8Array> | null): Promise<void> {
  if (!body) return;
  try {
    await body.cancel();
  } catch {
    // Ignore cleanup failures from probe requests.
  }
}

function limitBody(body: ReadableStream<Uint8Array>, limit: number): ReadableStream<Uint8Array> {
  let total = 0;
  const reader = body.getReader();
  return new ReadableStream<Uint8Array>({
    async pull(controller) {
      const { value, done } = await reader.read();
      if (done) {
        controller.close();
        return;
      }
      total += value.byteLength;
      if (total > limit) {
        controller.error(new Error('upstream body exceeded maxSize'));
        try { await reader.cancel(); } catch { /* ignore */ }
        return;
      }
      controller.enqueue(value);
    },
    async cancel(reason) {
      try { await reader.cancel(reason); } catch { /* ignore */ }
    },
  });
}
