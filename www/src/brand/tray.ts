// The app's tray icon, as art.rs draws it: "up" in a frame, on a 32px grid. Its letters sink as you
// do and turn over to read "dn" past the slouch limit, which the notches mark, so it reads without
// colour.

export type State = "upright" | "slouch" | "lost" | "paused";

/** The letters' top, as the app draws them at the start and end of the tray's range. */
const TOPS: Record<State, number> = { upright: 4, slouch: 17, lost: 8, paused: 8 };
/** Level with the letters' top at the slouch limit. */
const NOTCH_Y = 11.8;

/** The tray icon in the colour of the text around it. */
export function trayMark(state: State): string {
  const y = TOPS[state];
  const live = state === "upright" || state === "slouch";
  const dashes = state === "lost" ? ` stroke-dasharray="0.1 4.4"` : "";
  let letters = `<path d="M6 ${y} V${y + 6} a4 4 0 0 0 8 0 V${y}"${dashes}/><path d="M19 ${y + 16} V${y}"${dashes}/><circle cx="23" cy="${y + 5}" r="4"${dashes}/>`;
  if (state === "slouch") letters = `<g transform="rotate(180 16.5 ${y + 5})">${letters}</g>`;
  const notches = live ? `<path d="M1 ${NOTCH_Y} H3.2 M28.8 ${NOTCH_Y} H31" stroke-width="3"/>` : "";
  const frameOpacity = state === "lost" ? 0.6 : 1;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><defs><clipPath id="mark-inside"><rect x="1.7" y="1.7" width="28.6" height="28.6" rx="6.3"/></clipPath></defs><rect x="1" y="1" width="30" height="30" rx="7" stroke-width="1.4" opacity="${frameOpacity}"/>${notches}<g clip-path="url(#mark-inside)" stroke-width="3.2">${letters}</g></svg>`;
}
