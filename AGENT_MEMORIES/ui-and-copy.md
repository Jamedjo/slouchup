# UI & Copy

Rules for anything a person using SlouchUp sees: windows, the tray, notifications, installers, launcher entries and the homepage. Check a UI change against them before opening its pull request.

## Name

- **SlouchUp** in anything people see: window titles, the tray tooltip, notifications, the installer, Start menu and launcher entries. In code it's `APP_NAME`.
- **slouchup**, lowercase, only for technical identifiers: crate and binary names, the exe, package ids, folders and file paths (`APP_ID`). Never rename a path or id to match the display name.

## Voice

Warm, short, a little cheeky. The copy keeps attention on the person's posture, never on the camera.

- **Describe what people get, not what the app sees.** "Nudges you when your head drops", "shows how you're sitting". Never "watches", "monitors", "tracks", "sees you" or "keeps an eye on you".
- **Second person, about their posture.** "You're out of frame", not "I can't see you". "Your head has sunk lower than usual", not "I noticed you slouching".
- **Use the detector's own numbers instead of judgement.** "You're 32% closer to the screen than usual", not "Poor posture detected".
- **No creepiness reassurance.** Never "not creepy", "we're not spying", "nothing to worry about" or "we take your privacy seriously". Denying it plants the idea.
- **Privacy facts once, plainly, without adjectives,** where people look for them (first launch, settings, the homepage's privacy section): "Everything runs on your computer. Video never leaves it."
- **No internal notes in user copy.** An instruction meant for us ("don't read while I measure") is a design note, not something to ask of people.
- The calibration eyes are a target to look at, not a narrator: no lines, and never described as watching.

| Say | Not |
| --- | --- |
| Psst, sit up | Stop slouching! |
| You're out of frame. | Tracking failed. |
| SlouchUp is on | SlouchUp is watching |
| Sit up straight, look at the left screen. | Calibrating display 1 of 3 |

## Colour and type

Tokens are in `crates/slouch-app/src/tokens.css` (and `www/src/styles/tokens.css` for the homepage); `art.rs` carries the same hex values for drawn artwork. Use tokens, not hex, in CSS.

- **Day** theme (paper ground, ink text) for the History and Settings views; **Night** (ink ground, butter text) for the camera view and the calibration game. A window picks one with `data-theme="day"` or `"night"`.
- **butter**, **ink** and **tomato** are the brand. Butter and ink carry most of every surface; tomato is the accent.
- **tomato** is for the raised "up", shapes, fills and text of 24px or more.
- **tomato-text** for small tomato text on a light ground; **tomato-soft** for tomato on ink.
- **tomato-fill** behind small ink text, as on a tomato button. Plain tomato is too low contrast there.
- **sky** is for links and focus rings only, never a posture state.
- Posture states use **state-upright**, **state-slouch** and **state-lost**, which each theme sets to something legible.
- Fonts: Fredoka 600 for the wordmark (`--font-wordmark`), Figtree for headings and everything else (`--font-display`, `--font-body`). Both are built in, so the app never fetches fonts.

## Interaction

- **Keyboard alone and mouse alone must each reach everything.** A keyboard step also gets a big button, such as Next in calibration.
- **No window-wide single-key shortcuts** (C, P, Space…): they stop people typing into a box later. Keyboard access comes from Tab order, Enter or Space on the focused control, and keys scoped to the active control or an open menu or dialog (arrows, Home/End, Esc, type-ahead). A window-wide shortcut needs a modifier, and asking first.
- **Pause is a primary action.** People start meetings and need the camera back, so keep it high up.
- **Sliders over presets.** Advanced controls go in an expandable inside the section they belong to, not a separate grab-bag.
- **Centre the camera preview horizontally,** so looking at it lines up with a camera at the top centre of the screen.

## Screenshots

Only ever of the drawn demo person: run with `--demo`, or `scripts/screenshots.sh` for the README set. Never a real webcam.
