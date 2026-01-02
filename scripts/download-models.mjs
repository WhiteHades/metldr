#!/usr/bin/env node
// download-models.mjs - Downloads core ML models for bundling
// Run with: node scripts/download-models.mjs

import { existsSync, mkdirSync, writeFileSync, readFileSync } from 'fs'
import { join, dirname } from 'path'
import { fileURLToPath } from 'url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const MODELS_DIR = join(__dirname, '..', 'public', 'models')

// all models to bundle (3 core models = ~318MB)
const CORE_MODELS = [
  // embed - required for all semantic search and RAG
  {
    id: 'nomic-ai/nomic-embed-text-v1.5',
    files: [
      'config.json',
      'tokenizer.json',
      'tokenizer_config.json',
      'special_tokens_map.json',
      'onnx/model_quantized.onnx'  // q8 quantized (~131MB)
    ],
    localDir: 'nomic-embed-text-v1.5'
  },
  // classify - used for email categorization
  {
    id: 'MoritzLaurer/deberta-v3-xsmall-zeroshot-v1.1-all-33',
    files: [
      'config.json',
      'tokenizer.json',
      'tokenizer_config.json',
      'special_tokens_map.json',
      'onnx/model_quantized.onnx'  // (~83MB)
    ],
    localDir: 'deberta-v3-xsmall-zeroshot'
  },
  // ner - extract names, dates, organizations
  {
    id: 'Xenova/bert-base-NER',
    files: [
      'config.json',
      'tokenizer.json',
      'tokenizer_config.json',
      'onnx/model_quantized.onnx'  // (~104MB)
    ],
    localDir: 'bert-base-NER'
  }
]

const HF_BASE = 'https://huggingface.co'

async function downloadFile(url, dest) {
  console.log(`  Downloading: ${url}`)
  const res = await fetch(url)
  if (!res.ok) throw new Error(`Failed to download ${url}: ${res.status}`)
  const buffer = await res.arrayBuffer()
  writeFileSync(dest, Buffer.from(buffer))
  console.log(`  Saved: ${dest} (${(buffer.byteLength / 1024 / 1024).toFixed(2)} MB)`)
}

async function downloadModel(model) {
  console.log(`\nDownloading: ${model.id}`)
  const modelDir = join(MODELS_DIR, model.localDir)
  
  if (!existsSync(modelDir)) {
    mkdirSync(modelDir, { recursive: true })
  }
  
  for (const file of model.files) {
    const url = `${HF_BASE}/${model.id}/resolve/main/${file}`
    const localPath = join(modelDir, file) // keep onnx/ subfolder structure
    
    // create subdirs if needed
    const dir = dirname(localPath)
    if (!existsSync(dir)) mkdirSync(dir, { recursive: true })
    
    // skip if already exists
    if (existsSync(localPath)) {
      console.log(`  Skip (exists): ${localPath}`)
      continue
    }
    
    await downloadFile(url, localPath)
  }
}

async function main() {
  console.log('=== MeTLDR Model Downloader ===\n')
  console.log(`Target directory: ${MODELS_DIR}`)
  
  if (!existsSync(MODELS_DIR)) {
    mkdirSync(MODELS_DIR, { recursive: true })
  }
  
  for (const model of CORE_MODELS) {
    try {
      await downloadModel(model)
    } catch (err) {
      console.error(`Failed to download ${model.id}:`, err.message)
    }
  }
  
  console.log('\n=== Download complete ===')
  console.log('Models are ready in public/models/')
  console.log('Run `npm run build` to bundle them with the extension.')
}

main().catch(console.error)
