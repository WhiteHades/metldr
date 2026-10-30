import { build, context } from 'esbuild';
import { copyFile, mkdir, readFile, rm } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkReceipt, watchPaths } from './answer-contract.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const ui = join(root, 'extension/chat-ui');
const output = join(root, 'public/chat');

async function options(frame = false) {
  const { dependencies } = JSON.parse(await readFile(join(ui, 'package.json'), 'utf8'));
  const morphNotice = await readFile(join(ui, 'node_modules/idiomorph/LICENSE'), 'utf8');
  return {
    entryPoints: frame ? { 'answer-frame': join(ui, 'answer-frame.ts') } : { chat: join(ui, 'main.tsx') },
    outdir: output,
    entryNames: '[name]',
    chunkNames: 'chunks/[name]-[hash]',
    splitting: !frame,
    bundle: true,
    jsx: 'automatic',
    format: 'esm',
    platform: 'browser',
    target: 'chrome148',
    minify: true,
    metafile: true,
    legalComments: 'eof',
    banner: { js: `/*! idiomorph 0.3.0\n${morphNotice}\n*/` },
    define: { 'process.env.NODE_ENV': '"production"' },
    loader: { '.woff2': 'file', '.woff': 'file', '.ttf': 'file', '.wasm': 'binary' },
    plugins: [{ name: 'metldr-ui', setup(builder) {
      builder.onStart(() => { checkReceipt(); });
      builder.onEnd(async result => {
        if (result.errors.length) return;
        const runtime = Object.entries(result.metafile?.outputs ?? {}).find(([name]) => resolve(name) === join(output, 'answer-frame.js'))?.[1];
        if (runtime && runtime.imports.length) throw new Error('The saved answer runtime must be self contained.');
        checkReceipt();
        if (frame) await copyBindings();
        checkReceipt();
      });
      builder.onResolve({ filter: /\?worker-source$/ }, args => ({
        path: resolve(args.resolveDir, args.path.slice(0, -'?worker-source'.length)), namespace: 'metldr-worker-source',
      }));
      builder.onResolve({ filter: /\.wasm\?binary$/ }, args => ({
        path: resolve(args.resolveDir, args.path.slice(0, -'?binary'.length)),
      }));
      builder.onLoad({ filter: /.*/, namespace: 'metldr-worker-source' }, async args => {
        const worker = await build({ entryPoints: [args.path], bundle: true, write: false,
          format: 'iife', platform: 'browser', target: 'chrome148', minify: true, metafile: true,
          define: { 'process.env.NODE_ENV': '"production"' },
        });
        if (worker.outputFiles.length !== 1 || Object.values(worker.metafile.outputs).some(item => item.imports.length)) {
          throw new Error('The answer worker must be self contained.');
        }
        return { contents: `export default ${JSON.stringify(worker.outputFiles[0].text)}`, loader: 'js',
          watchFiles: Object.keys(worker.metafile.inputs).map(file => resolve(root, file)) };
      });
      builder.onResolve({ filter: /answer_codec\.js$/ }, async args => {
        if (args.pluginData === 'answer-contract-resolution') return;
        const result = await builder.resolve(args.path, {
          resolveDir: args.resolveDir, kind: args.kind, pluginData: 'answer-contract-resolution',
        });
        return { ...result, ...watchPaths() };
      });
      builder.onResolve({ filter: /metldr_panel\.js$/ }, () => ({ path: '/chat/wasm/metldr_panel.js', external: true, ...watchPaths() }));
      builder.onResolve({ filter: /^[^./]/ }, async args => {
        if (args.pluginData === 'canonical-dependency') return;
        const parts = args.path.split('/');
        const name = args.path.startsWith('@') ? parts.slice(0, 2).join('/') : parts[0];
        if (!Object.hasOwn(dependencies, name)) return;
        const result = await builder.resolve(args.path, {
          resolveDir: ui, kind: args.kind, pluginData: 'canonical-dependency',
        });
        return { ...result, pluginData: undefined };
      });
    } }],
  };
}

async function copyBindings() {
  const destination = join(output, 'wasm');
  await mkdir(destination, { recursive: true });
  for (const name of ['metldr_panel.js', 'metldr_panel_bg.wasm']) {
    await copyFile(join(root, 'crates/metldr-panel/pkg', name), join(destination, name));
  }
}

export async function buildChat() {
  checkReceipt();
  await rm(output, { recursive: true, force: true });
  const results = [await build(await options()), await build(await options(true))];
  checkReceipt();
  return { inputs: new Set(results.flatMap(result => Object.keys(result.metafile.inputs))).size,
    outputs: new Set(results.flatMap(result => Object.keys(result.metafile.outputs))).size };
}

export async function watchChat() {
  checkReceipt();
  await rm(output, { recursive: true, force: true });
  const compilers = [];
  try {
    compilers.push(await context(await options()));
    compilers.push(await context(await options(true)));
    for (const compiler of compilers) await compiler.rebuild();
    checkReceipt();
    for (const compiler of compilers) await compiler.watch();
    return async () => { await Promise.all(compilers.map(compiler => compiler.dispose())); };
  } catch (error) {
    await Promise.all(compilers.map(compiler => compiler.dispose()));
    throw error;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  console.log(JSON.stringify(await buildChat()));
}
