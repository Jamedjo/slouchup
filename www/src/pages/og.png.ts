import type { APIRoute } from "astro";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { Resvg } from "@resvg/resvg-js";
import satori from "satori";
import { card, HEIGHT, WIDTH } from "../brand/card";

const require = createRequire(import.meta.url);

/** Satori reads WOFF but not the WOFF2 the page itself loads. */
async function font(name: string, weight: 400 | 600) {
  const family = name.toLowerCase();
  const file = require.resolve(`@fontsource/${family}/files/${family}-latin-${weight}-normal.woff`);
  return { name, weight, data: await readFile(file) };
}

export const GET: APIRoute = async () => {
  const fonts = await Promise.all([font("Fredoka", 600), font("Figtree", 400), font("Figtree", 600)]);
  const svg = await satori(card(), { width: WIDTH, height: HEIGHT, fonts });
  const png = new Resvg(svg).render().asPng();
  return new Response(new Uint8Array(png), { headers: { "Content-Type": "image/png" } });
};
