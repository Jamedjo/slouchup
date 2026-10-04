# Commits & Pull Requests

How commit messages and pull requests are written here. A pull request's title is its commit's subject. When a pull request has several commits, its title names the feature they deliver together, and each commit keeps its own subject.

## Deriving a subject

Every subject, for a commit or a pull request, comes from these five steps. Show them as a **Subject derivation** list in the reply that proposes the commit: the five answers, then the candidate subject, then wait. The author decides by reading the derivation. Never commit a subject the author has not read.

1. **What was made possible or fixed?** Get this from the author, not the diff. A diff shows where the work landed, not what it was for. If the author isn't there, use the commit or pull request body they wrote, and say that's where it came from.
2. **What is the subject?** Name the feature first, then the part of it that changed. A part on its own ("buttons", "dark icon") doesn't say where in the app you are. Someone scanning the whole log should know the feature from the first word or two.
3. **What is the impact on that subject?**
4. **The subject line is (2) plus (3), concise.** Keep the part to about two words after the feature; a third usually brings a filler word with it.
5. **The how is usually left out**, because it is in the code. It goes in the body only when it is a gotcha that matters going forward.

Worked example, from the nudge on Windows:

1. Fixed nudges on Windows naming Windows PowerShell as their sender
2. Windows toasts
3. No longer shown as PowerShell
4. **`Fixed Windows toasts showing as PowerShell`**
5. `Fixed Windows toasts showing as PowerShell by registering an app ID` was rejected. The subject line alone is enough.

## Tests a subject must pass

These are checks to apply to a candidate, not a recipe that writes one. Several styles are valid, and the same change can have more than one good title.

- **The click-in test.** From the title alone, a reviewer should know what to open to see the change: a window, the tray, a notification, a command. If they can't, the title is describing reasoning rather than a change.
- **No invented nouns.** Use the words in the app's UI, the names in the code, and the author's own words from the conversation. A coined phrase ("sender identity", "nudge cadence", "posture moment") reads as precise only because it is unfamiliar, and the reader has to translate it before they can judge it.
- **Never personify.** A subject records a change; it doesn't describe a thing acting on its own. No inanimate subject with an animate verb (`Tray icon watches the taskbar`, `Nudge waits for an answer`). No stative or posture verbs standing in for location (`stands`, `sits`, `holds`, `lives`, `remains`). No thinking verbs, either as the verb or as a participle: `judged`, `checked`, `read`, `decided`, `considered`, `knows`, `sees`. Something has to be doing the judging, and that is mechanism. The test is whether the sentence implies an actor, whatever the tense. Participles of plain changes (`added`, `removed`, `renamed`, `shown`) are fine.
- **Fixes read as fixes.** When something was wrong, the subject opens with `Fixed` and names the fault (`Fixed Windows toasts showing as PowerShell`, not `Fixed a bug in Windows toasts`), or names the fault and ends in `fixed` (`Stretched camera picture fixed`).
- **Name the facet that was solved, not the container.** A security fix that touches a window is a security commit. `Fixed dark tray icon on a dark Windows taskbar` names what the work was *about*; `Registry lookup added to windows_shell` names where it landed.
- **The change, not where it sits.** Position is rarely what changed: `Snooze beside Settings in the tray menu` describes a layout. Say what someone can now do: `Nudge snooze added to the tray menu`.
- **The change, not its history.** `Deleted tests restored` puts the past in the subject; `Tests reintroduced` is the change.
- **One half per subject.** "and" or "with" holding two halves means one half is the subject and the other goes in the body, or it is two commits.
- **A technical subject is fine when the impact is technical.** Don't translate a refactor or a data change into a behaviour that undersells it: `Config and cache paths renamed to slouchup` was chosen over `Settings kept after the rename`.
- **Short.** About 50 characters is a soft limit, 72 the hard one. Hitting the limit by cramming in the wrong words doesn't count. A long subject usually means the change hasn't been distilled yet.
- No `feat:`/`fix:` prefixes, no arrows (`→`), no `#123` references, and nothing that reads like marketing copy.

## Examples

| Rejected | Fault | Preferred |
|---|---|---|
| Show a Snooze button on the macOS nudge | bare verb opener names nothing | Nudge snooze button added on macOS |
| Sender identity registered for toasts | invented noun; a fix not written as one | Fixed Windows toasts showing as PowerShell |
| Settings and cache live under slouchup | stative verb standing in for a change | Config and cache paths renamed to slouchup |
| Nudges can be snoozed for 30 minutes | a detail that belongs in the body | Nudge snooze added |
| Nudge icon shows the calibration game's eyes looking up, on tomato | one step of the work, not the change it made | App restyled to the slouchup design system |
| Tray and toasts fit in on Windows | personifies; two halves | Fixed dark tray icon on a dark Windows taskbar |

The last one held three changes. Split into three commits, each subject names its own fault or feature: `Fixed dark tray icon on a dark Windows taskbar`, `Fixed Windows toasts showing as PowerShell` and `Nudge buttons added on Windows`.

## Commit body

Choose one of two:

- **No body** for a small or self-explanatory change.
- **One line of why, then bullets of high-level what.** Never restate the diff. Don't let the why line repeat the subject. "Problem: solution" framing helps for significant bugfixes. Leave out implementation details, file lists, test counts and follow-up plans.

Keep sentences short (about 1.5 lines at most) and use bullets otherwise, because long messages don't get read. Drop project or app context the reader already has. The only implementation detail worth including is something unusual that future readers need to know about.

Each commit is a change in behaviour, not a step in the work. Fold fix-ups into the commit they fix and force push, so the history reads as what changed rather than how it was arrived at.

## Pull request descriptions

The description is the review document. The title is the subject, Why and What come from the commit message, and Try it is what a reviewer needs to see the change. Sections after What appear only when there is something to put in them.

```markdown
## Why
What someone needed, or what went wrong before. One short paragraph.

## What
One sentence on the change, then bullets only if it needs them.

## Try it
The command to run, and where to click or what to type to reach the change.

## Screenshots
| Before | After |
|--------|-------|
| ![](…) | ![](…) |

## Why not
Known limits, temporary fixes, alternatives turned down.

## Follow up
- [ ] Deferred work, struck through rather than deleted when dropped.
```

- Screenshots only for visible changes, as a table of before and after or of labelled states, taken with the demo person (`--demo`, or `scripts/screenshots.sh` for the README set). Use a short clip for animation or timing.
- A small fix needs one Why sentence and one What sentence. Never list files, restate the diff or count tests.
- No testing section and nothing else the template doesn't ask for.
- Questions for the author go to the author, not into the pull request.
