import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(fileURLToPath(new URL('..', import.meta.url)));
const crates = [
  { name: 'metldr-core', module: 'metldr_core', pkg: 'crates/metldr-core/pkg' },
  { name: 'metldr-welcome', module: 'welcome_runtime', pkg: 'public/welcome/pkg' },
];
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

for (const crate of crates) {
  const crateDir = join('crates', crate.name);
  const targetDir = join(crateDir, 'target');
  const cargoArgs = ['--locked', '--manifest-path', join(crateDir, 'Cargo.toml'), '--lib', '--target', 'wasm32-unknown-unknown'];
  if (isCheck) {
    run('cargo', ['check', ...cargoArgs], 'Cargo');
  } else {
    run('cargo', ['build', '--release', ...cargoArgs, '--target-dir', targetDir], 'Cargo');
    run('wasm-bindgen', [
      join(targetDir, 'wasm32-unknown-unknown', 'release', `${crate.module}.wasm`),
      '--target', 'web', '--typescript', '--out-dir', crate.pkg, '--out-name', crate.module,
    ], 'wasm-bindgen');
  }
}

run('cargo', [
  isCheck ? 'check' : 'build', '--locked', '--release',
  '--manifest-path', 'crates/metldr-welcome/Cargo.toml', '--bin', 'welcome-guide',
], 'welcome generator');
