import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, readFileSync, statSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { recordReceipt, sourceDigest } from './answer-contract.mjs';

const repoRoot = resolve(fileURLToPath(new URL('..', import.meta.url)));
const crates = [
  { name: 'metldr-core', module: 'metldr_core', pkg: 'crates/metldr-core/pkg' },
  { name: 'metldr-welcome', module: 'welcome_runtime', pkg: 'public/welcome/pkg' },
  { name: 'metldr-panel', module: 'metldr_panel', pkg: 'crates/metldr-panel/pkg' },
  { name: 'metldr-answer-codec', artifact: 'metldr_answer_codec', module: 'answer_codec', pkg: 'crates/metldr-answer-codec/pkg', features: 'guest' },
];
const isCheck = process.argv.includes('--check');
const isWindows = process.platform === 'win32';
const panelAssetFields = ['name', 'url', 'sha256', 'bytes'];

for (const asset of JSON.parse(readFileSync(join(repoRoot, 'scripts/panel-assets.json'), 'utf8'))) {
  if (Object.keys(asset).length !== panelAssetFields.length
      || !panelAssetFields.every(field => Object.hasOwn(asset, field))
      || typeof asset.url !== 'string') {
    throw new Error('Panel asset entries must contain name, url, sha256 and bytes.');
  }
  const { name, bytes, sha256 } = asset;
  const file = join(repoRoot, 'public/rust-panel/assets', name);
  if (basename(name) !== name || bytes > 256 * 1024 || statSync(file).size !== bytes
      || createHash('sha256').update(readFileSync(file)).digest('hex') !== sha256) {
    throw new Error(`Panel asset verification failed: ${name}`);
  }
}

function run(command, args, label) {
  const executable = isWindows ? `${command}.exe` : command;
  const result = spawnSync(executable, args, {
    cwd: repoRoot,
    stdio: 'inherit',
    shell: false,
  });

  if (result.error) {
    console.error(`Unable to run ${label} (${executable}): ${result.error.message}`);
    process.exit(result.error.code === 'ENOENT' ? 127 : 1);
  }
  if (result.status !== 0) {
    if (result.status === null) {
      console.error(`${label} terminated by signal ${result.signal ?? 'unknown'}`);
      process.exit(1);
    }
    process.exit(result.status);
  }
}

if (!isCheck) {
  const versionResult = spawnSync(isWindows ? 'wasm-bindgen.exe' : 'wasm-bindgen', ['--version'], {
    cwd: repoRoot,
    encoding: 'utf8',
    shell: false,
  });
  if (versionResult.error) {
    console.error(`Unable to run wasm-bindgen${isWindows ? '.exe' : ''}: ${versionResult.error.message}`);
    process.exit(versionResult.error.code === 'ENOENT' ? 127 : 1);
  }
  if (versionResult.status !== 0) process.exit(versionResult.status ?? 1);
  if (!/^wasm-bindgen 0\.2\.128(?:\s|$)/.test(versionResult.stdout.trim())) {
    console.error(`Expected wasm-bindgen 0.2.128, found: ${versionResult.stdout.trim() || 'unknown version'}`);
    process.exit(1);
  }

}

const answerSourceHash = isCheck ? undefined : sourceDigest();

for (const crate of crates) {
  const crateDir = join('crates', crate.name);
  const targetDir = process.env.CARGO_TARGET_DIR || join(crateDir, 'target');
  const cargoArgs = ['--locked', '--manifest-path', join(crateDir, 'Cargo.toml'), '--lib', '--target', 'wasm32-unknown-unknown'];
  if (crate.features) cargoArgs.push('--features', crate.features);
  if (isCheck) {
    run('cargo', ['check', ...cargoArgs], 'Cargo');
  } else {
    run('cargo', ['build', '--release', ...cargoArgs, '--target-dir', targetDir], 'Cargo');
    run('wasm-bindgen', [
      join(targetDir, 'wasm32-unknown-unknown', 'release', `${crate.artifact ?? crate.module}.wasm`),
      '--target', 'web', '--typescript', '--out-dir', crate.pkg, '--out-name', crate.module,
    ], 'wasm-bindgen');
  }
}

if (answerSourceHash) recordReceipt(answerSourceHash);

if (!isCheck) {
  const destination = join(repoRoot, 'public/chat/wasm');
  mkdirSync(destination, { recursive: true });
  for (const name of ['metldr_panel.js', 'metldr_panel_bg.wasm']) {
    copyFileSync(join(repoRoot, 'crates/metldr-panel/pkg', name), join(destination, name));
  }
}

run('cargo', [
  isCheck ? 'check' : 'build', '--locked', '--release',
  '--manifest-path', 'crates/metldr-welcome/Cargo.toml', '--bin', 'welcome-guide',
  '--target-dir', process.env.CARGO_TARGET_DIR || 'crates/metldr-welcome/target',
], 'welcome generator');

