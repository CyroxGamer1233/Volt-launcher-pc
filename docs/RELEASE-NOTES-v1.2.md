## Volt Launcher 1.2

Two things players kept seeing in the wrong place, and one new download.
Clean-up release: nothing on screen that was not asked for, and every enchanted
item glowing the way it should.

### Fixed

- **No more text over the top of the screen.** The stray caption at the top left
  was a *button label* imported from a touch layout — another launcher names its
  buttons with a description ("Sprinting (Key)") where Volt expects the key's own
  name. A caption that describes a bind instead of naming it is no longer painted,
  the button shows its key instead, and a button that draws no fill and no border
  no longer paints its caption at all. Sound subtitles are not drawn anywhere, so
  nothing else can put a line of text there.
- **The enchantment glint shows on every enchanted item.** The glint used to
  depend on the item model resolving an enchantment component, so some weapons
  stayed dull while others glinted. Every stack is now checked with its own
  `hasFoil` (enchantments, or the item's glint override) and any layer that came
  back without a glint gets one — flat models, shields and tridents included.
- **The pause menu has Mouse and Back, plus LMB and RMB.** On a phone the menu's
  side buttons were empty on the touch HUD; now all four are there, so you can
  click, go back, and use a real mouse button without a hardware mouse.
- **The hotbar slides.** Dragging a thumb along the hotbar scrolls through it
  instead of only jumping between the nine built-in slots.
- **"Request max refresh rate" requests a real rate.** The option left the panel
  at its default; it now asks the display for its highest rate and reports it.
- **The V badge can be dismissed and comes back when it should**, instead of
  leaving you without the launcher shortcut.

### New download

- **Universal APK.** One file that contains all four ABIs (arm64-v8a,
  armeabi-v7a, x86_64, x86) next to the existing per-ABI files, for anyone who
  does not want to pick a build. It is about 242 MB because the native libraries
  of all four are inside; every per-ABI file is still attached and a third of
  that size.

### Notes

- This release is built from the same commit as the rolling debug build, so the
  test channel and the release page cannot disagree.
- Installing 1.2 over 1.1.x updates in place: same application id, same signing
  certificate, higher version code. No uninstall, no lost worlds, no reset
  settings.
- Nothing here changes an option you already set: no setting is added, removed or
  reset, and a 1.1.1 save loads as-is.
