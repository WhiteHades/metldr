import { chunk_for_embedding } from '../../../crates/metldr-core/pkg/metldr_core'
import { loadRustCore } from '../RustCore'

export type ContentType = 'email' | 'article' | 'pdf'

export interface ChunkResult {
  text: string
  index: number
  tokenCount: number
  start: number
  end: number
}

export async function chunkForEmbedding(text: string, contentType: ContentType = 'article'): Promise<ChunkResult[]> {
  await loadRustCore()

  const chunks: ChunkResult[] = JSON.parse(chunk_for_embedding(text, contentType))
  return chunks
}

export const chunkingService = { chunkForEmbedding }
