import { build, context } from 'esbuild';
import { copyFile, mkdir, readFile, rm } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const ui = join(root, 'extension/chat-ui');
const output = join(root, 'public/chat');

async function options() {
  const { dependencies } = JSON.parse(await readFile(join(ui, 'package.json'), 'utf8'));
  return {
    entryPoints: [join(ui, 'main.tsx')],
    outdir: output,
    entryNames: 'chat',
    chunkNames: 'chunks/[name]-[hash]',
    splitting: true,
    bundle: true,
    jsx: 'automatic',
    format: 'esm',
    platform: 'browser',
    target: 'chrome148',
    minify: true,
    metafile: true,
    legalComments: 'eof',
    define: { 'process.env.NODE_ENV': '"production"' },
    loader: { '.woff2': 'file', '.woff': 'file', '.ttf': 'file' },
    plugins: [{ name: 'metldr-ui', setup(builder) {
      builder.onResolve({ filter: /metldr_panel\.js$/ }, () => ({ path: '/chat/wasm/metldr_panel.js', external: true }));
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
  await rm(output, { recursive: true, force: true });
  const result = await build(await options());
  await copyBindings();
  return { inputs: Object.keys(result.metafile.inputs).length, outputs: Object.keys(result.metafile.outputs).length };
}

export async function watchChat() {
  await rm(output, { recursive: true, force: true });
  const compiler = await context(await options());
  try {
    await compiler.rebuild();
    await copyBindings();
    await compiler.watch();
    return () => compiler.dispose();
  } catch (error) {
    await compiler.dispose();
    throw error;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  console.log(JSON.stringify(await buildChat()));
}
