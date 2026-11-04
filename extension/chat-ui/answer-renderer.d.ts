interface MetldrAnswerRenderer {
  updateDocument(html: string, css: string, complete: boolean): string[];
}

declare const metldrCreateAnswerRenderer: (options: {
  insert(html: string): void;
  onRendered(complete: boolean): void;
}) => MetldrAnswerRenderer;
