import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const receiptPath = join(root, 'crates/metldr-answer-codec/pkg/answer-codec.receipt.json');
const outputs = [
  'crates/metldr-answer-codec/pkg/answer_codec.js',
  'crates/metldr-answer-codec/pkg/answer_codec.d.ts',
  'crates/metldr-answer-codec/pkg/answer_codec_bg.wasm',
  'crates/metldr-panel/pkg/metldr_panel.js',
  'crates/metldr-panel/pkg/metldr_panel.d.ts',
  'crates/metldr-panel/pkg/metldr_panel_bg.wasm',
];

const crates = ['metldr-answer-codec', 'metldr-conversation', 'metldr-history-pages', 'metldr-panel'];

function sourceFiles(path, directories = false) {
  const absolute = join(root, path);
  if (!existsSync(absolute)) return [];
  return readdirSync(absolute, { withFileTypes: true }).flatMap(entry => {
    const child = join(path, entry.name);
    if (entry.isDirectory()) return [...(directories ? [child] : []), ...sourceFiles(child, directories)];
    return entry.isFile() && !directories ? [child] : [];
  }).sort();
}

function inputFiles() {
  return ['rust-toolchain.toml', ...crates.flatMap(name => {
    const base = join('crates', name);
    const build = join(base, 'build.rs');
    return [join(base, 'Cargo.toml'), join(base, 'Cargo.lock'),
      ...(existsSync(join(root, build)) ? [build] : []),
      ...sourceFiles(join(base, 'src')), ...sourceFiles(join(base, 'assets'))];
  })];
}

function hashFiles(files) {
  const hash = createHash('sha256');
  for (const path of [...files].sort()) {
    const absolute = join(root, path);
    if (!existsSync(absolute)) throw new Error(`Missing answer codec input/output: ${path}`);
    hash.update(relative(root, absolute));
    hash.update('\0');
    hash.update(readFileSync(absolute));
    hash.update('\0');
  }
  return hash.digest('hex');
}

export function sourceDigest() {
  return hashFiles(inputFiles());
}

export function watchPaths() {
  return {
    watchFiles: [...inputFiles(), ...outputs, relative(root, receiptPath)].map(path => join(root, path)),
    watchDirs: crates.flatMap(name => {
      const base = join('crates', name);
      return [base, ...['src', 'assets'].flatMap(dir => {
        const source = join(base, dir);
        return existsSync(join(root, source)) ? [source, ...sourceFiles(source, true)] : [];
      })];
    }).map(path => join(root, path)),
  };
}

function outputDigests() {
  return Object.fromEntries(outputs.map(path => [path, hashFiles([path])]));
}

export function recordReceipt(expectedSourceHash) {
  const beforeOutputs = sourceDigest();
  if (expectedSourceHash !== beforeOutputs) {
    throw new Error('Answer codec sources changed during the Rust build; refusing to record outputs.');
  }
  const outputHashes = outputDigests();
  const actualSourceHash = sourceDigest();
  if (expectedSourceHash !== actualSourceHash) {
    throw new Error('Answer codec sources changed while recording Rust outputs.');
  }
  const receipt = {
    version: 1,
    sourceHash: actualSourceHash,
    outputs: outputHashes,
  };
  writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`);
  return receipt;
}

export function checkReceipt() {
  if (!existsSync(receiptPath)) throw new Error('Answer codec build receipt is missing; run the Rust build first.');
  const receipt = JSON.parse(readFileSync(receiptPath, 'utf8'));
  const beforeOutputs = sourceDigest();
  if (receipt.version !== 1 || receipt.sourceHash !== beforeOutputs) {
    throw new Error('Answer codec sources changed since the recorded Rust build; rebuild Rust outputs.');
  }
  const actual = outputDigests();
  if (sourceDigest() !== beforeOutputs) {
    throw new Error('Answer codec sources changed during the build freshness check.');
  }
  if (JSON.stringify(receipt.outputs) !== JSON.stringify(actual)) {
    throw new Error('Answer codec or panel bindings changed since the recorded Rust build; rebuild Rust outputs.');
  }
  return true;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const mode = process.argv[2];
    if (mode === 'check') checkReceipt();
    else if (mode === 'source-hash') console.log(sourceDigest());
    else if (mode === 'record' && process.argv[3]) recordReceipt(process.argv[3]);
    else throw new Error('Usage: answer-contract.mjs check | source-hash | record <source-hash>');
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
