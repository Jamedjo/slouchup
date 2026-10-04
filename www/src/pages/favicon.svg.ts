import type { APIRoute } from "astro";
import { upTile } from "../brand/up";

export const GET: APIRoute = () =>
  new Response(upTile("app"), { headers: { "Content-Type": "image/svg+xml" } });
