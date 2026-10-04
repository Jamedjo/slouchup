// The app's demo person in their room, from the same drawing the README screenshots show.

import drawing from "../../../crates/slouch-app/src/person.svg?raw";

export type Pose = "upright" | "slump" | "lean";

/** How far each pose sinks and how close it comes, as the app's demo acts them out. */
const shapes: Record<Pose, { sink: number; scale: number }> = {
  upright: { sink: 0, scale: 1 },
  slump: { sink: 80, scale: 1 },
  lean: { sink: 40, scale: 1.4 },
};

/** The eye line of the upright pose, in the drawing's 640×480 frame. */
export const BASELINE = 234;

export function person(pose: Pose): string {
  const { sink, scale } = shapes[pose];
  return drawing.replace("{cy}", String(240 + sink)).replace("{scale}", String(scale));
}
