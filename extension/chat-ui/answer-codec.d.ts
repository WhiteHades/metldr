declare module "*.wasm?binary" {
  const bytes: Uint8Array<ArrayBuffer>;
  export default bytes;
}
