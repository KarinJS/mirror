type Bucket = 'gh' | 'raw' | 'avatar' | 'unpkg' | 'mirror' | 'other';

const counters: Record<Bucket, number> = {
  gh: 0, raw: 0, avatar: 0, unpkg: 0, mirror: 0, other: 0,
};

let total = 0;
let startedAt = Date.now();

export function bumpRoute(bucket: Bucket): void {
  counters[bucket] += 1;
  total += 1;
}

export function statsSnapshot() {
  return {
    uptimeMs: Date.now() - startedAt,
    total,
    perRoute: { ...counters },
  };
}

export function bucketFromPath(pathname: string): Bucket {
  const seg = pathname.split('/', 2)[1] ?? '';
  if (seg === 'gh') return 'gh';
  if (seg === 'raw') return 'raw';
  if (seg === 'avatar') return 'avatar';
  if (seg === 'unpkg') return 'unpkg';
  if (seg === 'mirror') return 'mirror';
  return 'other';
}
