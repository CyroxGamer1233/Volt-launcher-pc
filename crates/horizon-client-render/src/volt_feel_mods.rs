//! Motion Blur and HitReg: the two visual-feel Volt mods.
//!
//! Both are pure state machines with no GPU or game dependency, so the rules
//! can be tested directly. The game feeds them camera angles and hit results
//! and draws whatever they report.

use std::time::{Duration, Instant};

/// How long a hit-marker stays on screen.
pub const HIT_MARKER: Duration = Duration::from_millis(180);
/// Hits slower than this register as late; the readout turns amber.
pub const HIT_SLOW_MS: f32 = 120.0;
/// Hits this late are shown in coral.
pub const HIT_LATE_MS: f32 = 250.0;

/// Motion Blur: how far the camera turned between two frames, in degrees.
///
/// Vanilla measures nothing between frames, so this reads the same `yaw` and
/// `pitch` the renderer already draws from. A strength of 0 keeps every frame
/// perfectly sharp; larger strengths let the smear grow at lower turn speeds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MotionBlur {
    last: Option<(f32, f32)>,
    /// Turn magnitude of the frame that just ended, in degrees.
    velocity: f32,
}

impl MotionBlur {
    /// Feed the camera angles drawn this frame and get the blur radius,
    /// 0.0 (sharp) to 1.0 (maximum smear).
    ///
    /// The first frame has no previous angle to compare against, so it is
    /// always sharp: a blur there would look like a stutter on world load.
    pub fn frame(&mut self, yaw: f32, pitch: f32, strength: f32) -> f32 {
        if !yaw.is_finite() || !pitch.is_finite() {
            self.last = None;
            self.velocity = 0.0;
            return 0.0;
        }
        let Some((last_yaw, last_pitch)) = self.last.replace((yaw, pitch)) else {
            self.velocity = 0.0;
            return 0.0;
        };
        // Yaw wraps at 360, so the short way round is the real turn: whipping
        // past due north must not read as a full revolution.
        let mut dyaw = (yaw - last_yaw) % 360.0;
        if dyaw > 180.0 {
            dyaw -= 360.0;
        } else if dyaw < -180.0 {
            dyaw += 360.0;
        }
        let dpitch = pitch - last_pitch;
        self.velocity = dyaw.hypot(dpitch);
        self.radius(strength)
    }

    /// The blur radius for the turn measured by the last [`Self::frame`].
    pub fn radius(&self, strength: f32) -> f32 {
        if !strength.is_finite() || strength <= 0.0 || self.velocity <= 0.0 {
            return 0.0;
        }
        // A fast flick is roughly 12 degrees in a frame. Dividing by that
        // makes an ordinary look barely blur and a whip saturate.
        let scaled = self.velocity / 12.0 * strength;
        scaled.clamp(0.0, 1.0)
    }

    /// The turn of the last frame, in degrees. Exposed for the debug overlay.
    pub fn velocity(&self) -> f32 {
        self.velocity
    }

    /// Forget the previous angle, so the next frame is sharp again. Called
    /// when the camera jumps rather than turning (teleport, respawn, a
    /// perspective switch), which would otherwise smear across the cut.
    pub fn reset(&mut self) {
        self.last = None;
        self.velocity = 0.0;
    }
}

/// One confirmed hit, as the crosshair readout needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitMarker {
    /// How long the click took to resolve into a hit.
    pub latency: Duration,
    at: Instant,
}

impl HitMarker {
    /// 1.0 right after the hit, fading to 0.0 when the marker expires.
    pub fn alpha(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.at);
        if elapsed >= HIT_MARKER {
            return 0.0;
        }
        let left = HIT_MARKER.saturating_sub(elapsed).as_secs_f32();
        (left / HIT_MARKER.as_secs_f32()).clamp(0.0, 1.0)
    }

    pub fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.at) >= HIT_MARKER
    }

    pub fn millis(&self) -> f32 {
        self.latency.as_secs_f32() * 1000.0
    }

    /// Whether this hit was slow enough to colour differently.
    pub fn is_slow(&self) -> bool {
        self.millis() >= HIT_SLOW_MS
    }

    /// Whether this hit was late enough to read as a connection problem.
    pub fn is_late(&self) -> bool {
        self.millis() >= HIT_LATE_MS
    }
}

/// HitReg: confirms attacks and reports how long each took to land.
///
/// The clock starts when the attack is issued and stops when the game reports
/// the hit, so the number is the real click-to-confirm latency rather than a
/// guess from the ping.
#[derive(Clone, Copy, Debug, Default)]
pub struct HitReg {
    pending_since: Option<Instant>,
    marker: Option<HitMarker>,
    hits: u32,
}

impl HitReg {
    /// An attack pressed this frame; the next [`Self::confirm`] times it.
    pub fn attack(&mut self, now: Instant) {
        self.pending_since = Some(now);
    }

    /// The attack landed. Returns the marker to draw, or `None` when no
    /// attack was outstanding (so a stray hit cannot fake a latency).
    pub fn confirm(&mut self, now: Instant) -> Option<HitMarker> {
        let since = self.pending_since.take()?;
        let marker = HitMarker {
            latency: now.saturating_duration_since(since),
            at: now,
        };
        self.marker = Some(marker);
        self.hits = self.hits.saturating_add(1);
        Some(marker)
    }

    /// The attack resolved without hitting: no marker, no stored latency.
    pub fn miss(&mut self) {
        self.pending_since = None;
    }

    pub fn marker(&self) -> Option<HitMarker> {
        self.marker
    }

    /// Drops the marker once it has faded, so nothing is drawn forever.
    pub fn expire(&mut self, now: Instant) {
        if self.marker.is_some_and(|m| m.expired(now)) {
            self.marker = None;
        }
    }

    /// Hits confirmed since the mod was switched on, for the HUD counter.
    pub fn hits(&self) -> u32 {
        self.hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_frame_is_sharp_and_a_still_camera_stays_sharp() {
        let mut blur = MotionBlur::default();
        assert_eq!(blur.frame(10.0, 5.0, 1.0), 0.0);
        // Same angle again: no movement, no smear.
        assert_eq!(blur.frame(10.0, 5.0, 1.0), 0.0);
        assert_eq!(blur.velocity(), 0.0);
    }

    #[test]
    fn turning_blurs_more_the_faster_it_turns() {
        let mut blur = MotionBlur::default();
        blur.frame(0.0, 0.0, 1.0);
        let gentle = blur.frame(2.0, 0.0, 1.0);
        blur.reset();
        blur.frame(0.0, 0.0, 1.0);
        let flick = blur.frame(12.0, 0.0, 1.0);
        assert!(gentle > 0.0 && gentle < 0.5, "gentle {gentle}");
        assert!(flick > gentle, "{flick} vs {gentle}");
        assert!(flick <= 1.0);
    }

    #[test]
    fn yaw_wraps_the_short_way_round() {
        let mut blur = MotionBlur::default();
        blur.frame(359.0, 0.0, 1.0);
        // A 2 degree turn across due north, not a 358 degree spin.
        blur.frame(1.0, 0.0, 1.0);
        assert!(blur.velocity() <= 2.5, "velocity {}", blur.velocity());
    }

    #[test]
    fn zero_strength_never_blurs() {
        let mut blur = MotionBlur::default();
        blur.frame(0.0, 0.0, 0.0);
        assert_eq!(blur.frame(90.0, 40.0, 0.0), 0.0);
    }

    #[test]
    fn teleport_resets_to_sharp() {
        let mut blur = MotionBlur::default();
        blur.frame(0.0, 0.0, 1.0);
        assert!(blur.frame(90.0, 45.0, 1.0) > 0.0);
        blur.reset();
        // The jump frame is sharp because there is nothing to compare to.
        assert_eq!(blur.frame(180.0, 0.0, 1.0), 0.0);
    }

    #[test]
    fn hit_latency_needs_an_attack_in_flight() {
        let now = Instant::now();
        let mut hit = HitReg::default();
        // A hit with no attack pressed is ignored rather than reported as 0ms.
        assert!(hit.confirm(now).is_none());
        assert_eq!(hit.hits(), 0);

        hit.attack(now);
        let marker = hit
            .confirm(now + Duration::from_millis(42))
            .expect("hit confirmed");
        assert!((marker.millis() - 42.0).abs() < 1.0);
        assert!(!marker.is_slow());
        assert_eq!(hit.hits(), 1);
    }

    #[test]
    fn a_miss_clears_the_pending_attack() {
        let now = Instant::now();
        let mut hit = HitReg::default();
        hit.attack(now);
        hit.miss();
        assert!(hit.confirm(now + Duration::from_millis(30)).is_none());
    }

    #[test]
    fn slow_hits_are_flagged_and_the_marker_fades_away() {
        let now = Instant::now();
        let mut hit = HitReg::default();
        hit.attack(now);
        let marker = hit.confirm(now + Duration::from_millis(300)).unwrap();
        assert!(marker.is_slow());
        assert!(marker.is_late());
        assert!((marker.alpha(now + Duration::from_millis(300)) - 1.0).abs() < 0.01);
        assert!(marker.alpha(now + Duration::from_millis(300) + HIT_MARKER) == 0.0);

        hit.expire(now + Duration::from_millis(300) + HIT_MARKER);
        assert!(hit.marker().is_none());
    }
}
