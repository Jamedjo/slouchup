// The OpenGraph card, as the nodes satori draws: what slouchup does for you, beside the nudge
// that does it.

import { BUTTER, INK, PAPER, SAND, STONE, TOMATO, TOMATO_FILL, TOMATO_SOFT } from "./colours";
import { upTile } from "./up";

export const WIDTH = 1200;
export const HEIGHT = 630;
export const HEADLINE = "Better posture without thinking about it.";
export const SUBHEAD = "One gentle nudge when you start to slouch, then it leaves you alone.";
export const ALT = `SlouchUp. ${HEADLINE} Beside it, a nudge: Psst, sit up.`;

type Style = Record<string, string | number>;

export interface Node {
  type: string;
  props: { style: Style; children?: (Node | string)[] | string; src?: string; width?: number; height?: number };
}

const node = (type: string, style: Style, children?: Node["props"]["children"]): Node => ({
  type,
  props: { style, children },
});

const display = (size: number): Style => ({ fontFamily: "Figtree", fontWeight: 800, fontSize: size, lineHeight: 1.05, letterSpacing: -0.025 * size });
const wordmarkFace = (size: number): Style => ({ fontFamily: "Fredoka", fontWeight: 600, fontSize: size, lineHeight: 1.05 });

/** "up" raised and coloured as the wordmark has it, in the wordmark's face. */
function up(colour: string, size: number): Node {
  return node("span", { position: "relative", top: -0.25 * size, color: colour, ...wordmarkFace(size) }, "up");
}

function wordmark(): Node {
  return node("div", { display: "flex", alignItems: "baseline", color: INK, ...wordmarkFace(48) }, ["slouch", up(TOMATO, 48)]);
}

function tile(size: number): Node {
  const src = `data:image/svg+xml;base64,${btoa(upTile("nudge"))}`;
  return { type: "img", props: { style: { flexShrink: 0 }, src, width: size, height: size } };
}

function button(label: string, filled: boolean): Node {
  const colours = filled
    ? { background: TOMATO_FILL, borderColor: TOMATO_FILL, color: INK }
    : { background: "transparent", borderColor: BUTTER, color: BUTTER };
  return node(
    "div",
    { display: "flex", alignItems: "center", height: 64, padding: "0 28px", border: "3px solid", borderRadius: 999, fontFamily: "Figtree", fontWeight: 600, fontSize: 24, ...colours },
    label,
  );
}

/** The nudge as the app shows it, at one and a half times its size on screen. */
function nudge(): Node {
  return node(
    "div",
    { display: "flex", gap: 22, width: 560, padding: 26, borderRadius: 26, background: INK, boxShadow: `9px 9px 0 ${TOMATO}`, color: BUTTER },
    [
      tile(66),
      node("div", { display: "flex", flexDirection: "column", gap: 10 }, [
        node("div", { display: "flex", alignItems: "baseline", gap: 9, paddingTop: 8, ...display(34) }, ["Psst, sit", up(TOMATO_SOFT, 34)]),
        node("div", { fontFamily: "Figtree", fontSize: 22, lineHeight: 1.45, color: SAND }, "Your head has sunk lower than usual."),
        node("div", { display: "flex", gap: 12, marginTop: 6 }, [button("I'm up", true), button("Pause 30 min", false)]),
      ]),
    ],
  );
}

export function card(): Node {
  return node("div", { display: "flex", position: "relative", width: WIDTH, height: HEIGHT, background: PAPER }, [
    node("div", { position: "absolute", left: 700, top: 40, width: 560, height: 550, borderRadius: 28, background: SAND }),
    node("div", { position: "absolute", left: 64, top: 64, display: "flex" }, [wordmark()]),
    node("div", { position: "absolute", left: 64, top: 0, bottom: 0, width: 520, display: "flex", flexDirection: "column", justifyContent: "center", gap: 24 }, [
      node("div", { color: INK, ...display(64) }, HEADLINE),
      node("div", { fontFamily: "Figtree", fontSize: 30, lineHeight: 1.35, color: STONE }, SUBHEAD),
    ]),
    node("div", { position: "absolute", left: 590, top: 0, bottom: 0, display: "flex", alignItems: "center" }, [nudge()]),
  ]);
}
