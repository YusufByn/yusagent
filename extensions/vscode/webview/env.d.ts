declare module "*.css";

/** Imported as text (esbuild `text` loader): the SVG markup. */
declare module "*.svg" {
  const svg: string;
  export default svg;
}
