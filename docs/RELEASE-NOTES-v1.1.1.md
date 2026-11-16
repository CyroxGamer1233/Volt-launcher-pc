## Volt Launcher 1.1.1

Three Android problems reported by players, all fixed: crouching opened the
shield, the top of every screen tangled with the phone's status bar, and a key
bind picked on the Controls page never took the key you tapped.

### Fixed

- **Crouching no longer raises the shield.** With the separate Shield button
  turned off (the default), the game treated the crouch control as "raise the
  shield" — and because a touch crouch is a stick click, every crouch opened the
  shield over whatever you were doing. Vanilla does the opposite: a shield in
  the off hand is suppressed while sneaking. A raise is now only ever the Shield
  button when it is on the HUD, or the Use press when it is not. Standing still,
  crouching, or tapping crouch with a sword in hand no longer changes your
  guard.
- **No more corrupted text under the status bar.** From Android 15 the system
  draws the status bar over the app whether the app asks it to or not, so the
  "Volt Mods" title bar and the Mouse/Back corner buttons were rendered in the
  same rows as the clock, the wifi icon and the battery, and the glyphs
  interleaved. The window now hides the status and navigation bars the way a
  full-screen game is meant to, comes back after the keyboard or a permission
  dialog takes focus, and asks the display cutout to keep clear of its edges.
- **Tapping a key bind now sets it.** On a phone the only keys a tap can produce
  are the ones on the touch HUD's key grid, and that grid starts collapsed
  behind "..." on the header — with the status bar sitting on top of it. A row
  waiting for a key now opens the grid automatically, so the keys are on screen
  and one tap finishes the bind. The grid also stays open for the whole capture
  instead of closing after the first key. For a binding that wants a mouse
  button, turn on **Mouse** in the header and tap **LMB** or **RMB**.

### Notes

- Nothing here changes settings you already have: no option is added, removed or
  reset, and a save file from 1.1.0 loads as-is.
- The status bar fix is the reason the whole screen looks slightly taller on
  Android 15 and 16: the bars are gone rather than overlapping, so every menu
  keeps its full layout.
