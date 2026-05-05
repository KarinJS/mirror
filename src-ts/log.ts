type Level = 'debug' | 'info' | 'warn' | 'error';

const RANK: Record<Level, number> = { debug: 10, info: 20, warn: 30, error: 40 };

function envLevel(): Level {
  const v = (process.env.LOG_LEVEL ?? 'info').toLowerCase();
  return (['debug', 'info', 'warn', 'error'].includes(v) ? v : 'info') as Level;
}

let minLevel: Level = envLevel();

export function setLogLevel(level: Level): void {
  minLevel = level;
}

function emit(level: Level, msg: string, fields?: Record<string, unknown>): void {
  if (RANK[level] < RANK[minLevel]) return;
  const line = JSON.stringify({
    t: new Date().toISOString(),
    level,
    msg,
    ...fields,
  });
  if (level === 'error' || level === 'warn') console.error(line);
  else console.log(line);
}

export const log = {
  debug: (msg: string, f?: Record<string, unknown>) => emit('debug', msg, f),
  info: (msg: string, f?: Record<string, unknown>) => emit('info', msg, f),
  warn: (msg: string, f?: Record<string, unknown>) => emit('warn', msg, f),
  error: (msg: string, f?: Record<string, unknown>) => emit('error', msg, f),
};
