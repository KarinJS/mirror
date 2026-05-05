export interface GeoConfig {
  mode: 'off' | 'allow' | 'deny';
  headerName: string;
  countries: string[];
}

export interface MirrorRuntimeConfig {
  defaultTTL: number;
  defaultMaxSize: number;
  absoluteMaxSize: number;
  fetchTimeoutMs: number;
}

export interface RouteCacheTTLConfig {
  raw: number;
  avatar: number;
  unpkg: number;
}

export interface AppConfig {
  host: string;
  port: number;
  publicOrigin: string;
  trustProxyHeaders: boolean;
  logLevel: 'debug' | 'info' | 'warn' | 'error';
  geo: GeoConfig;
  cacheTTL: RouteCacheTTLConfig;
  mirror: MirrorRuntimeConfig;
  cors: { enabledRoutes: string[] };
}

/** github.avatar.json: ["karinjs", ...] */
export type AvatarWhitelist = string[];

/** github.raw.json: { owner: { repo: [{ branch, file }] } } */
export type RawWhitelist = Record<
  string,
  Record<string, Array<{ branch: string; file: string }>>
>;

/** github.releases.json: { owner: { repo: [file, ...] } } */
export type ReleasesWhitelist = Record<string, Record<string, string[]>>;

/** unpkg.json: { pkg: [file, ...] } */
export type UnpkgWhitelist = Record<string, string[]>;

/** mirror.json: { url: number | { ttl, maxSize? } } */
export type MirrorRule = number | { ttl: number; maxSize?: number };
export type MirrorWhitelist = Record<string, MirrorRule>;

export interface Whitelists {
  avatar: AvatarWhitelist;
  raw: RawWhitelist;
  releases: ReleasesWhitelist;
  unpkg: UnpkgWhitelist;
  mirror: MirrorWhitelist;
}
