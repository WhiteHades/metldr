import { spawnSync } from 'node:child_process';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(fileURLToPath(new URL('..', import.meta.url)));
const crateDir = join(repoRoot, 'crates', 'metldr-core');
const manifestPath = join(crateDir, 'Cargo.toml');
const targetDir = join(crateDir, 'target');
const wasmPath = join(targetDir, 'wasm32-unknown-unknown', 'release', 'metldr_core.wasm');
const pkgDir = join(crateDir, 'pkg');
const isCheck = process.argv.includes('--check');
const isWindows = process.platform === 'win32';

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

const cargoArgs = ['--locked', '--manifest-path', relative(repoRoot, manifestPath), '--target', 'wasm32-unknown-unknown'];

if (isCheck) {
  run('cargo', ['check', ...cargoArgs], 'Cargo');
} else {
  run('cargo', ['build', '--release', ...cargoArgs, '--target-dir', relative(repoRoot, targetDir)], 'Cargo');

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

  run('wasm-bindgen', [
    relative(repoRoot, wasmPath),
    '--target', 'web',
    '--typescript',
    '--out-dir', relative(repoRoot, pkgDir),
    '--out-name', 'metldr_core',
  ], 'wasm-bindgen');
}
