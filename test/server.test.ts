import { afterAll, beforeAll, beforeEach, describe, expect, test } from 'bun:test';
import { handle } from '../src/server.ts';
import { loadAll } from '../src/config.ts';

type FetchCall = { target: string; init?: RequestInit };

const originalFetch = globalThis.fetch;
const fetchCalls: FetchCall[] = [];

let fetchResponder: (target: string, init?: RequestInit) => Response | Promise<Response> = defaultFetchResponder;

beforeAll(async () => {
  await loadAll();
});

beforeEach(() => {
  fetchCalls.length = 0;
  fetchResponder = defaultFetchResponder;
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    const target = typeof input === 'string'
      ? input
      : input instanceof URL
        ? input.toString()
        : input.url;
    fetchCalls.push({ target, init });
    return await fetchResponder(target, init);
  }) as typeof fetch;
});

afterAll(() => {
  globalThis.fetch = originalFetch;
});

function req(path: string, init?: RequestInit): Request {
  return new Request(`http://localhost${path}`, init);
}

function lastFetch(): FetchCall {
  const call = fetchCalls.at(-1);
  if (!call) throw new Error('expected fetch to be called');
  return call;
}

function defaultFetchResponder(target: string): Response {
  return new Response(`proxied:${target}`, {
    status: 200,
    headers: {
      'content-type': 'application/octet-stream',
      'content-disposition': 'inline; filename="asset.bin"',
      etag: '"upstream"',
      'last-modified': 'Mon, 01 Jan 2024 00:00:00 GMT',
    },
  });
}

describe('meta routes', () => {
  test('healthz', async () => {
    const r = await handle(req('/healthz'));
    expect(r.status).toBe(200);
    expect(await r.text()).toBe('ok');
  });
  test('robots.txt forbids all', async () => {
    const r = await handle(req('/robots.txt'));
    expect(r.status).toBe(200);
    expect(await r.text()).toContain('Disallow: /');
  });
  test('stats returns json', async () => {
    const r = await handle(req('/stats'));
    expect(r.status).toBe(200);
    const j = (await r.json()) as { perRoute: Record<string, number> };
    expect(j.perRoute).toBeDefined();
  });
});

describe('method & query', () => {
  test('POST returns 405', async () => {
    const r = await handle(req('/avatar/karinjs', { method: 'POST' }));
    expect(r.status).toBe(405);
  });
  test('query is stripped via 308', async () => {
    const r = await handle(req('/gh/NapNeko/NapCatQQ/releases/download/v4/NapCat.Framework.zip?x=1'));
    expect(r.status).toBe(308);
    expect(r.headers.get('location')).toBe('/gh/NapNeko/NapCatQQ/releases/download/v4/NapCat.Framework.zip');
  });
});

describe('avatar', () => {
  test('bare user path returns 404', async () => {
    const r = await handle(req('/avatar/karinjs'));
    expect(r.status).toBe(404);
    expect(fetchCalls).toHaveLength(0);
  });
  test('with .png suffix', async () => {
    fetchResponder = (target) => new Response(`proxied:${target}`, {
      status: 200,
      headers: {
        'content-type': 'image/png',
        'content-encoding': 'zstd',
        'content-length': '999',
      },
    });

    const r = await handle(req('/avatar/karinjs.png'));
    expect(r.status).toBe(200);
    expect(r.headers.get('location')).toBeNull();
    expect(r.headers.get('cache-control')).toBe('public, max-age=300');
    expect(r.headers.get('content-encoding')).toBeNull();
    expect(r.headers.get('content-length')).toBeNull();
    expect(await r.text()).toBe('proxied:https://github.com/karinjs.png');
    expect(lastFetch()).toEqual({
      target: 'https://github.com/karinjs.png',
      init: expect.objectContaining({
        method: 'GET',
        redirect: 'follow',
        headers: expect.any(Headers),
      }),
    });
    expect((lastFetch().init?.headers as Headers).get('accept-encoding')).toBe('identity');
  });
  test('non-whitelisted returns 404', async () => {
    const r = await handle(req('/avatar/some-other-user'));
    expect(r.status).toBe(404);
    expect(fetchCalls).toHaveLength(0);
  });
});

describe('releases', () => {
  test('whitelisted file with arbitrary tag', async () => {
    const r = await handle(req('/gh/NapNeko/NapCatQQ/releases/download/v4.18.0/NapCat.Framework.zip'));
    expect(r.status).toBe(200);
    expect(r.headers.get('cache-control')).toBe('public, max-age=31536000, immutable');
    expect(r.headers.get('content-disposition')).toBe('inline; filename="asset.bin"');
    expect(await r.text()).toBe(
      'proxied:https://github.com/NapNeko/NapCatQQ/releases/download/v4.18.0/NapCat.Framework.zip',
    );
  });
  test('non-whitelisted file 404', async () => {
    const r = await handle(req('/gh/NapNeko/NapCatQQ/releases/download/v4/foo.zip'));
    expect(r.status).toBe(404);
  });
  test('non-whitelisted repo 404', async () => {
    const r = await handle(req('/gh/foo/bar/releases/download/v1/x.zip'));
    expect(r.status).toBe(404);
  });
});

describe('raw', () => {
  test('HEAD branch wildcard matches main', async () => {
    const r = await handle(req('/raw/karinjs/karin/main/package.json'));
    expect(r.status).toBe(200);
    expect(r.headers.get('cache-control')).toBe('public, max-age=300');
    expect(r.headers.get('access-control-allow-origin')).toBe('*');
    expect(await r.text()).toBe('proxied:https://raw.githubusercontent.com/karinjs/karin/main/package.json');
  });
  test('non-whitelisted file 404', async () => {
    const r = await handle(req('/raw/karinjs/karin/main/secret.env'));
    expect(r.status).toBe(404);
  });
});

describe('unpkg', () => {
  test('versionless file redirects to canonical mirror path', async () => {
    fetchResponder = (target, init) => {
      if (target === 'https://unpkg.com/karin/package.json' && init?.method === 'HEAD' && init.redirect === 'manual') {
        return new Response(null, {
          status: 302,
          headers: { location: 'https://unpkg.com/karin@1.2.3/package.json' },
        });
      }
      if (target === 'https://unpkg.com/karin@1.2.3/package.json' && init?.method === 'HEAD' && init.redirect === 'manual') {
        return new Response(null, { status: 200 });
      }
      return defaultFetchResponder(target);
    };

    const r = await handle(req('/unpkg/karin/package.json'));
    expect(r.status).toBe(302);
    expect(r.headers.get('location')).toBe('/unpkg/karin@1.2.3/package.json');
    expect(r.headers.get('cache-control')).toBe('public, max-age=300');
  });

  test('whitelisted file with version', async () => {
    fetchResponder = (target, init) => {
      if (target === 'https://unpkg.com/karin@1.2.3/package.json' && init?.method === 'HEAD' && init.redirect === 'manual') {
        return new Response(null, { status: 200 });
      }
      return defaultFetchResponder(target);
    };

    const r = await handle(req('/unpkg/karin@1.2.3/package.json'));
    expect(r.status).toBe(200);
    expect(r.headers.get('cache-control')).toBe('public, max-age=300');
    expect(r.headers.get('access-control-allow-origin')).toBe('*');
    expect(await r.text()).toBe('proxied:https://unpkg.com/karin@1.2.3/package.json');
  });
  test('whitelisted file without version', async () => {
    fetchResponder = (target, init) => {
      if (target === 'https://unpkg.com/karin/dist/karin.umd.js' && init?.method === 'HEAD' && init.redirect === 'manual') {
        return new Response(null, {
          status: 302,
          headers: { location: 'https://unpkg.com/karin@1.2.3/dist/karin.umd.js' },
        });
      }
      if (target === 'https://unpkg.com/karin@1.2.3/dist/karin.umd.js' && init?.method === 'HEAD' && init.redirect === 'manual') {
        return new Response(null, { status: 200 });
      }
      return defaultFetchResponder(target);
    };

    const r = await handle(req('/unpkg/karin/dist/karin.umd.js'));
    expect(r.status).toBe(302);
    expect(r.headers.get('location')).toBe('/unpkg/karin@1.2.3/dist/karin.umd.js');
  });
  test('non-whitelisted file 404', async () => {
    fetchResponder = (target, init) => {
      if (target === 'https://unpkg.com/karin/dist/secret.js' && init?.method === 'HEAD' && init.redirect === 'manual') {
        return new Response(null, {
          status: 302,
          headers: { location: 'https://unpkg.com/karin@1.2.3/dist/secret.js' },
        });
      }
      if (target === 'https://unpkg.com/karin@1.2.3/dist/secret.js' && init?.method === 'HEAD' && init.redirect === 'manual') {
        return new Response(null, { status: 200 });
      }
      return defaultFetchResponder(target);
    };

    const r = await handle(req('/unpkg/karin/dist/secret.js'));
    expect(r.status).toBe(404);
  });
});

describe('mirror', () => {
  test('non-whitelisted target 404', async () => {
    const r = await handle(req('/mirror/example.com/whatever'));
    expect(r.status).toBe(404);
  });
});

describe('upstream failures', () => {
  test('returns 502 when upstream fetch throws', async () => {
    fetchResponder = () => {
      throw new Error('boom');
    };

    const r = await handle(req('/avatar/karinjs.png'));
    expect(r.status).toBe(502);
  });
});

describe('unknown path', () => {
  test('returns 404', async () => {
    const r = await handle(req('/totally-unknown'));
    expect(r.status).toBe(404);
  });
});
