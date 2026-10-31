//! The two buttons as the three actions of the reader. The shell reads whether each button is down
//! every few milliseconds and calls [`Buttons::update`] with the time; nothing here touches the
//! hardware, so the host tests drive it with a made-up clock.
//!
//! Button A is [`Action::Down`] on a click and [`Action::Up`] on a hold, like the Arduino sketch's
//! `wasClicked` and `wasHold`; button B is [`Action::Select`] on a click. A press shorter than
//! [`MIN_PRESS_MS`] is a bounce and is ignored. A hold fires once, as soon as it is long enough,
//! and the release that follows does nothing. While both buttons are down nothing fires, and
//! nothing does until both are up again, so pressing the two together by accident does no harm.

use crate::reader::Action;

/// How long button A must stay down to count as a hold, in milliseconds.
pub const HOLD_MS: u64 = 600;
/// Shorter presses are contact bounce.
pub const MIN_PRESS_MS: u64 = 40;

/// A button that is down: since when, and whether its hold has fired.
#[derive(Debug, Clone, Copy)]
struct Down {
    since: u64,
    fired: bool,
}

/// The state of both buttons.
#[derive(Debug, Clone, Copy, Default)]
pub struct Buttons {
    a: Option<Down>,
    b: Option<Down>,
    /// Both were down together: ignore everything until both are up.
    ignore: bool,
}

impl Buttons {
    /// Starts with the buttons as they are now. A button already down, one held while the board
    /// started, does nothing until it has been let go.
    pub fn new(a_down: bool, b_down: bool) -> Self {
        Self {
            a: None,
            b: None,
            ignore: a_down || b_down,
        }
    }

    /// Feeds the state of the buttons at time `now_ms` and returns the action it completes, if any.
    /// At most one action per call, so call it often (every 10 to 20 ms).
    pub fn update(&mut self, a_down: bool, b_down: bool, now_ms: u64) -> Option<Action> {
        if a_down && b_down {
            self.ignore = true;
        }
        if self.ignore {
            self.a = None;
            self.b = None;
            if !a_down && !b_down {
                self.ignore = false;
            }
            return None;
        }

        // Button A: a hold fires while it is down, a click when it comes up.
        match (&mut self.a, a_down) {
            (None, true) => {
                self.a = Some(Down {
                    since: now_ms,
                    fired: false,
                })
            }
            (Some(a), true) => {
                if !a.fired && now_ms.saturating_sub(a.since) >= HOLD_MS {
                    a.fired = true;
                    return Some(Action::Up);
                }
            }
            (Some(_), false) => {
                if let Some(a) = self.a.take()
                    && !a.fired
                    && now_ms.saturating_sub(a.since) >= MIN_PRESS_MS
                {
                    return Some(Action::Down);
                }
            }
            (None, false) => {}
        }

        // Button B: a click when it comes up.
        match (&self.b, b_down) {
            (None, true) => {
                self.b = Some(Down {
                    since: now_ms,
                    fired: false,
                })
            }
            (Some(_), false) => {
                if let Some(b) = self.b.take()
                    && now_ms.saturating_sub(b.since) >= MIN_PRESS_MS
                {
                    return Some(Action::Select);
                }
            }
            _ => {}
        }
        None
    }
}
