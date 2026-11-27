// Incrementa o PATCH da versão do app (não é a do Git) e grava data/hora de São Paulo.
// Uso: node scripts/bump-version.mjs [--check]   (--check só mostra a versão atual)
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const path = join(root, 'version.json');
const cur = JSON.parse(readFileSync(path, 'utf8'));
if (process.argv.includes('--check')) {
  console.log(`v${cur.version}${cur.built ? ' · ' + cur.built : ''}`);
  process.exit(0);
}
const [maj, min, pat] = cur.version.split('.').map(Number);
const version = `${maj}.${min}.${pat + 1}`;
const p = Object.fromEntries(
  new Intl.DateTimeFormat('pt-BR', { timeZone: 'America/Sao_Paulo', day: '2-digit', month: '2-digit', year: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false })
    .formatToParts(new Date()).map((x) => [x.type, x.value]),
);
const built = `${p.day}/${p.month}/${p.year} ${p.hour}:${p.minute}`;
writeFileSync(path, JSON.stringify({ version, built }, null, 2) + '\n');

const sub = (file, re, to) => {
  const f = join(root, file);
  if (!existsSync(f)) return;
  const s = readFileSync(f, 'utf8');
  const n = s.replace(re, to);
  if (n !== s) writeFileSync(f, n);
};
sub('package.json', /("version":\s*")[^"]+(")/, `$1${version}$2`);
sub('desktop/tauri.conf.json', /("version":\s*")[^"]+(")/, `$1${version}$2`);
sub('desktop/Cargo.toml', /^(version\s*=\s*")[^"]+(")/m, `$1${version}$2`);
sub('core/Cargo.toml', /^(version\s*=\s*")[^"]+(")/m, `$1${version}$2`);
console.log(`v${version} · ${built}`);
