import type { APIRoute } from "astro";
import appIcon from "../../../crates/slouch-app/src/app-icon.svg?raw";

export const GET: APIRoute = () =>
  new Response(appIcon, { headers: { "Content-Type": "image/svg+xml" } });
