## Volt Launcher 1.1.0

A fix-and-polish release: Freelook and the crouch button both work on every
device now, and Volt Boost gains an option for builders who light their world
with framed items.

### Fixed

- **Freelook works on touch.** When you turned the camera on a phone, the game
  adjusted the player's view directly and Freelook never saw a single finger
  movement, so the mod did nothing on Android. Touch look now orbits the camera
  the same way the mouse does, and the orbit runs at the correct speed instead
  of one-seventh of it. A new **Look** button on the touch HUD flips the mod on
  and off, since a touch button cannot hold a key down.
- **The crouch button crouches on the first tap.** In D-pad and tap-to-interact
  modes the sneaking button used to ignore the first tap and wait for a second
  one within half a second, so on those devices it read as a dead button. One
  tap now toggles crouching on every scheme, matching the joystick and a
  controller's stick click. Flying and swimming still use the same button as a
  held descend control, where it does not toggle.

### New

- **Horizontal and Vertical sensitivity** in Volt Settings → Controls. Each
  slider moves the camera on its own axis only, from 10% to 300%, so you can
  turn fast sideways while keeping vertical aim steady, or the other way round.
  The combined Touch Sensitivity still scales both at once.
- **Frame Items Loader** in Volt Boost. Item frames and glow item frames are
  normally drawn only within about 1024 blocks and only when they are in view.
  With this on, framed items are drawn from any distance and from any angle,
  which is what you want for a map wall, a trophy room, or a signpost grid you
  read from the far side of the world. Only item frames are affected; every
  other entity keeps its usual culling, so the cost is just the frames you
  actually placed. It is off by default.

### Notes

- Freelook and Frame Items Loader are both off until you switch them on, so an
  existing install behaves exactly as before after updating.
- **Menus no longer clamp to 60 FPS.** Every GUI used to be capped at 60 no
  matter what Max Framerate said, which is why turning Dynamic FPS off changed
  nothing in menus. A menu now honours your setting up to 60, so 30 stays 30 and
  120 stays 120. The idle cap for unattended menus still applies.
- Frame Items Loader draws every loaded frame in the world. On a server with
  thousands of frames spread over a long distance, expect the frames to cost
  GPU time in proportion to how many you have placed.
