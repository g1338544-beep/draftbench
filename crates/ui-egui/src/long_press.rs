//! Press and hold a finger on a touch screen = right click (owner: on a phone there is no right
//! button, so the context menus could not be opened).
//!
//! One finger held still for [`HOLD_S`] seconds lets go of the left press it started (late
//! enough not to count as a click) and clicks the right button where it is; lifting the finger
//! afterwards releases nothing more. A second finger (pinch, pan) or a move of more than
//! [`SLOP_PX`] points is a gesture, not a hold.

/// How long the finger must stay (seconds): past egui's longest click, so the left press it
/// lets go of is not a click.
pub const HOLD_S: f64 = 0.85;
/// How far the finger may wander and still be holding (points).
pub const SLOP_PX: f32 = 10.0;

#[derive(Default, Debug)]
pub struct LongPress {
    /// The finger down: its touch id, where it went down, when.
    down: Option<(u64, egui::Pos2, f64)>,
    /// The hold already right-clicked: the finger's left release is swallowed.
    fired: bool,
    /// Fingers on the screen now.
    fingers: usize,
}

impl LongPress {
    /// A finger is held and the next frames must come even without new input.
    pub fn waiting(&self) -> bool {
        self.down.is_some() && !self.fired
    }

    /// Read this frame's touches and, after a hold, add the right click to the input.
    pub fn hook(&mut self, raw: &mut egui::RawInput) {
        let now = raw.time.unwrap_or(0.0);
        for e in &raw.events {
            if let egui::Event::Touch { id, phase, pos, .. } = e {
                match phase {
                    egui::TouchPhase::Start => {
                        self.fingers += 1;
                        self.down = if self.fingers == 1 { Some((id.0, *pos, now)) } else { None };
                        if self.fingers == 1 {
                            self.fired = false;
                        }
                    }
                    egui::TouchPhase::Move => {
                        if self.down.is_some_and(|(d, p, _)| d == id.0 && p.distance(*pos) > SLOP_PX) {
                            self.down = None;
                        }
                    }
                    egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                        self.fingers = self.fingers.saturating_sub(1);
                        if self.down.is_some_and(|(d, _, _)| d == id.0) {
                            self.down = None;
                        }
                    }
                }
            }
        }
        // The left release of a finger that already right-clicked: it was let go of already.
        if self.fired {
            raw.events.retain(|e| !matches!(e, egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: false, .. }));
            if self.fingers == 0 {
                self.fired = false;
            }
        }
        if let Some((_, pos, at)) = self.down
            && !self.fired
            && now - at >= HOLD_S
        {
            let none = egui::Modifiers::NONE;
            raw.events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: none });
            raw.events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed: true, modifiers: none });
            raw.events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed: false, modifiers: none });
            self.fired = true;
            self.down = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(id: u64, phase: egui::TouchPhase, x: f32) -> egui::Event {
        egui::Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(id), phase, pos: egui::pos2(x, 50.0), force: None }
    }

    fn frame(lp: &mut LongPress, time: f64, events: Vec<egui::Event>) -> Vec<egui::Event> {
        let mut raw = egui::RawInput { time: Some(time), events, ..Default::default() };
        lp.hook(&mut raw);
        raw.events
    }

    fn right_clicks(events: &[egui::Event]) -> usize {
        events.iter().filter(|e| matches!(e, egui::Event::PointerButton { button: egui::PointerButton::Secondary, pressed: false, .. })).count()
    }

    #[test]
    fn a_held_finger_right_clicks_once_and_its_lift_releases_nothing_more() {
        let mut lp = LongPress::default();
        let press = egui::Event::PointerButton { pos: egui::pos2(50.0, 50.0), button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE };
        frame(&mut lp, 0.0, vec![touch(7, egui::TouchPhase::Start, 50.0), press]);
        assert!(lp.waiting());
        assert_eq!(right_clicks(&frame(&mut lp, 0.5, vec![])), 0, "not yet");
        let held = frame(&mut lp, 0.9, vec![]);
        assert_eq!(right_clicks(&held), 1, "held: a right click");
        assert!(held.iter().any(|e| matches!(e, egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: false, .. })), "the left press is let go of");
        assert_eq!(right_clicks(&frame(&mut lp, 1.5, vec![])), 0, "once");
        let release = egui::Event::PointerButton { pos: egui::pos2(50.0, 50.0), button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE };
        let lifted = frame(&mut lp, 2.0, vec![touch(7, egui::TouchPhase::End, 50.0), release]);
        assert!(!lifted.iter().any(|e| matches!(e, egui::Event::PointerButton { pressed: false, .. })), "nothing more released: {lifted:?}");
        // The next tap is an ordinary tap again.
        let tap = egui::Event::PointerButton { pos: egui::pos2(50.0, 50.0), button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE };
        frame(&mut lp, 3.0, vec![touch(8, egui::TouchPhase::Start, 50.0)]);
        let up = frame(&mut lp, 3.1, vec![touch(8, egui::TouchPhase::End, 50.0), tap]);
        assert_eq!(up.len(), 2, "a quick tap passes through: {up:?}");
    }

    #[test]
    fn a_moving_finger_or_two_fingers_are_a_gesture_not_a_hold() {
        let mut lp = LongPress::default();
        frame(&mut lp, 0.0, vec![touch(1, egui::TouchPhase::Start, 50.0)]);
        frame(&mut lp, 0.2, vec![touch(1, egui::TouchPhase::Move, 80.0)]);
        assert_eq!(right_clicks(&frame(&mut lp, 1.2, vec![])), 0, "dragged: orbit, not a menu");
        frame(&mut lp, 1.3, vec![touch(1, egui::TouchPhase::End, 80.0)]);
        frame(&mut lp, 2.0, vec![touch(2, egui::TouchPhase::Start, 50.0)]);
        frame(&mut lp, 2.1, vec![touch(3, egui::TouchPhase::Start, 90.0)]);
        assert_eq!(right_clicks(&frame(&mut lp, 3.5, vec![])), 0, "two fingers: pinch or pan");
    }
}
