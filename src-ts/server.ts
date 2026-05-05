import { extname, join, resolve } from 'node:path';
import { getConfig, loadAll, startWatchers } from './config.ts';
import { checkGeo } from './geo.ts';
import {
  clientCountry,
  clientIp,
  geoBlocked,
  methodNotAllowed,
  notFound,
  plain,
  redirectStripQuery,
  withCors,
} from './http.ts';
import { log, setLogLevel } from './log.ts';
import { handleAvatar } from './routes/avatar.ts';
import { handleMirror } from './routes/mirror.ts';
import { handleRaw } from './routes/raw.ts';
import { handleReleases } from './routes/releases.ts';
import { handleUnpkg } from './routes/unpkg.ts';
import { bucketFromPath, bumpRoute, statsSnapshot } from './stats.ts';

const DIST_DIR = resolve(import.meta.dir, '..', 'webui', 'dist');

const MIME_TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'application/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.webp': 'image/webp',
};

async function serveStatic(pathname: string): Promise<Response | null> {
  const filePath = join(DIST_DIR, pathname);
  const file = Bun.file(filePath);
  if (!(await file.exists())) return null;
  const ext = extname(filePath);
  const contentType = MIME_TYPES[ext] ?? 'application/octet-stream';
  const cache = ext === '.html' ? 'no-cache' : 'public, max-age=31536000, immutable';
  return new Response(file, { headers: { 'content-type': contentType, 'cache-control': cache } });
}

const INDEX_HTML = `<!doctype html><meta charset="utf-8"><title>mirror.karinjs.com</title>
<style>body{font-family:system-ui,sans-serif;max-width:720px;margin:6vh auto;padding:0 1rem;color:#222}code{background:#f3f3f3;padding:.1em .35em;border-radius:3px}</style>
<h1>mirror.karinjs.com</h1>
<p>Whitelist-based reverse proxy. Routes:</p>
<ul>
<li><code>/gh/&lt;owner&gt;/&lt;repo&gt;/releases/download/&lt;tag&gt;/&lt;file&gt;</code></li>
<li><code>/raw/&lt;owner&gt;/&lt;repo&gt;/&lt;branch&gt;/&lt;path&gt;</code></li>
<li><code>/avatar/&lt;user&gt;.png</code></li>
<li><code>/unpkg/&lt;pkg&gt;[@version]/&lt;file&gt;</code></li>
<li><code>/mirror/&lt;host&gt;/&lt;path&gt;</code></li>
</ul>
<p>Source: <a href="https://github.com/karinjs">github.com/karinjs</a></p>`;

const ROBOTS_TXT = 'User-agent: *\nDisallow: /\n';

export async function buildHandler(): Promise<(req: Request) => Promise<Response> | Response> {
  await loadAll();
  setLogLevel(getConfig().logLevel);
  startWatchers();
  return handle;
}

export async function handle(req: Request): Promise<Response> {
  const startedAt = performance.now();
  const cfg = getConfig();
  const url = new URL(req.url);

  let res: Response;
  let upstream: string | undefined;
  try {
    res = await route(req, url, cfg);
  } catch (err) {
    log.error('handler crashed', { err: String(err), path: url.pathname });
    res = plain(500, 'Internal Server Error');
  }

  const elapsedMs = +(performance.now() - startedAt).toFixed(1);
  log.info('req', {
    ip: clientIp(req, cfg.trustProxyHeaders),
    country: clientCountry(req, cfg.geo.headerName),
    method: req.method,
    path: url.pathname,
    status: res.status,
    elapsedMs,
    upstream,
  });

  return res;
}

async function route(req: Request, url: URL, cfg: ReturnType<typeof getConfig>): Promise<Response> {
  // Health & meta endpoints — bypass query strip & method filter.
  if (url.pathname === '/healthz') return plain(200, 'ok');
  if (url.pathname === '/robots.txt') return plain(200, ROBOTS_TXT);
  if (url.pathname === '/stats') {
    return new Response(JSON.stringify(statsSnapshot()), {
      status: 200,
      headers: { 'content-type': 'application/json; charset=utf-8' },
    });
  }

  // Method filter (allow OPTIONS for CORS preflight on whitelisted routes).
  if (req.method === 'OPTIONS') {
    return withCors(new Response(null, { status: 204 }));
  }
  if (req.method !== 'GET' && req.method !== 'HEAD') {
    return methodNotAllowed();
  }

  // Geo gate.
  if (checkGeo(cfg.geo, req.headers) === 'deny') return geoBlocked();

  // Query strip — applies to ALL functional routes.
  if (url.search !== '') return redirectStripQuery(url.pathname);

  // Static files from webui/dist (hashed assets, fonts, etc.).
  const staticRes = await serveStatic(url.pathname);
  if (staticRes) return staticRes;

  // Proxy routes.
  const bucket = bucketFromPath(url.pathname);
  bumpRoute(bucket);

  let res: Response | null = null;
  switch (bucket) {
    case 'gh':
      res = await handleReleases(url, req);
      break;
    case 'raw':
      res = await handleRaw(url, req);
      break;
    case 'avatar':
      res = await handleAvatar(url, req);
      break;
    case 'unpkg':
      res = await handleUnpkg(url, req);
      break;
    case 'mirror':
      res = await handleMirror(url, req);
      break;
    default:
      res = null;
  }

  if (res !== null) {
    if (cfg.cors.enabledRoutes.includes(bucket)) return withCors(res);
    return res;
  }

  // SPA fallback — serve webui/dist/index.html for client-side routing.
  const spaRes = await serveStatic('/index.html');
  if (spaRes) return spaRes;

  // Inline fallback when webui has not been built.
  return new Response(INDEX_HTML, {
    status: 200,
    headers: { 'content-type': 'text/html; charset=utf-8' },
  });
}

if (import.meta.main) {
  const handler = await buildHandler();
  const cfg = getConfig();
  Bun.serve({
    hostname: cfg.host,
    port: cfg.port,
    fetch: handler,
    error(err) {
      log.error('server error', { err: String(err) });
      return new Response('Internal Server Error', { status: 500 });
    },
  });
  log.info('listening', { host: cfg.host, port: cfg.port });
}
