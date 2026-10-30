declare module "*.css" {}

declare module "*?worker-source" {
  const source: string;
  export default source;
}
