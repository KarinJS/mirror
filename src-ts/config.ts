import { watch } from 'node:fs';
import { resolve } from 'node:path';
import type {
  AppConfig,
  AvatarWhitelist,
  MirrorWhitelist,
  RawWhitelist,
  ReleasesWhitelist,
  UnpkgWhitelist,
  Whitelists,
} from './types.ts';
import { log } from './log';

const ROOT = resolve(import.meta.dir, '..');
const CONFIG_ROOT = resolve(ROOT, 'config');

const FILES = {
  config: 'config.json',
  avatar: 'github.avatar.json',
  raw: 'github.raw.json',
  releases: 'github.releases.json',
  unpkg: 'unpkg.json',
  mirror: 'mirror.json',
} as const;

async function readJson<T>(name: string): Promise<T> {
  const file = Bun.file(resolve(CONFIG_ROOT, name));
  return (await file.json()) as T;
}

let config: AppConfig;
const whitelists: Whitelists = {
  avatar: [],
  raw: {},
  releases: {},
  unpkg: {},
  mirror: {},
};

export function getConfig(): AppConfig {
  return config;
}

export function getWhitelists(): Whitelists {
  return whitelists;
}

async function reloadConfig(): Promise<void> {
  try {
    config = await readJson<AppConfig>(FILES.config);
    log.info('config reloaded', { port: config.port, geoMode: config.geo.mode });
  } catch (err) {
    log.error('failed to reload config', { err: String(err) });
  }
}

async function reloadWhitelist<K extends keyof Whitelists>(key: K): Promise<void> {
  try {
    const data = await readJson<Whitelists[K]>(FILES[key]);
    whitelists[key] = data;
    log.info('whitelist reloaded', { which: key });
  } catch (err) {
    log.error('failed to reload whitelist', { which: key, err: String(err) });
  }
}

export async function loadAll(): Promise<void> {
  await reloadConfig();
  await Promise.all([
    reloadWhitelist('avatar'),
    reloadWhitelist('raw'),
    reloadWhitelist('releases'),
    reloadWhitelist('unpkg'),
    reloadWhitelist('mirror'),
  ]);
}

export function startWatchers(): void {
  const debounce = new Map<string, ReturnType<typeof setTimeout>>();
  const schedule = (file: string, fn: () => void) => {
    const prev = debounce.get(file);
    if (prev) clearTimeout(prev);
    debounce.set(
      file,
      setTimeout(() => {
        debounce.delete(file);
        fn();
      }, 150),
    );
  };

  for (const [key, file] of Object.entries(FILES)) {
    const abs = resolve(CONFIG_ROOT, file);
    try {
      watch(abs, () => {
        schedule(file, () => {
          if (key === 'config') void reloadConfig();
          else void reloadWhitelist(key as keyof Whitelists);
        });
      });
    } catch (err) {
      log.warn('watch failed', { file, err: String(err) });
    }
  }
}
