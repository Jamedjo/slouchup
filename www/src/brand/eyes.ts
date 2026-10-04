// The tray's eyes on a 24px grid. Their shape says how you're sitting, so they read without colour.

export type Mood = "upright" | "slouch" | "lost";

const shapes: Record<Mood, string> = {
  upright: `<path d="M3 13a4.5 4.5 0 1 0 9 0a4.5 4.5 0 1 0 -9 0M12 13a4.5 4.5 0 1 0 9 0a4.5 4.5 0 1 0 -9 0"/>
<circle cx="7.5" cy="10.6" r="1.7" fill="currentColor" stroke="none"/>
<circle cx="16.5" cy="10.6" r="1.7" fill="currentColor" stroke="none"/>`,
  slouch: `<circle cx="7.5" cy="12" r="4.5"/><circle cx="16.5" cy="12" r="4.5"/>
<path d="M3 10.4h9M12 10.4h9"/>
<circle cx="7.5" cy="14.3" r="1.7" fill="currentColor" stroke="none"/>
<circle cx="16.5" cy="14.3" r="1.7" fill="currentColor" stroke="none"/>`,
  lost: `<path d="M3 13c1.6 2.2 7.4 2.2 9 0M12 13c1.6 2.2 7.4 2.2 9 0"/>
<path d="M17.4 3.6a1.7 1.7 0 1 1 2.4 1.6c-.5.3-.8.6-.8 1.2"/>
<circle cx="19" cy="8.6" r="0.6" fill="currentColor" stroke="none"/>`,
};

/** The eyes in the colour of the text around them. */
export function eyes(mood: Mood): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${shapes[mood]}</svg>`;
}
