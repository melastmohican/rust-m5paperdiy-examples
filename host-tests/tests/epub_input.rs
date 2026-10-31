//! Tests of `common/src/input.rs`: the two buttons as the four actions of the reader, with a made-up
//! clock polled every 20 ms like the board does.

use m5paperdiy_common::input::{Buttons, HOLD_MS, MIN_PRESS_MS};
use m5paperdiy_common::reader::Action::{self, Down, Select, Skip, Up};

/// What the buttons do over time: `(from_ms, a_down, b_down)`, each state lasting until the next entry.
/// Returns every action with the time it happened.
fn run(start: (bool, bool), script: &[(u64, bool, bool)], until_ms: u64) -> Vec<(u64, Action)> {
    let mut buttons = Buttons::new(start.0, start.1);
    let mut state = start;
    let mut out = Vec::new();
    let mut next = script.iter().peekable();
    let mut now = 0;
    while now <= until_ms {
        while let Some(&&(at, a, b)) = next.peek() {
            if at > now {
                break;
            }
            state = (a, b);
            next.next();
        }
        if let Some(action) = buttons.update(state.0, state.1, now) {
            out.push((now, action));
        }
        now += 20;
    }
    out
}

fn actions(events: &[(u64, Action)]) -> Vec<Action> {
    events.iter().map(|e| e.1).collect()
}

#[test]
fn a_click_of_a_is_down_when_it_is_let_go() {
    let events = run(
        (false, false),
        &[(100, true, false), (220, false, false)],
        1000,
    );
    assert_eq!(events, [(220, Down)]);
}

#[test]
fn a_click_of_b_is_select_when_it_is_let_go() {
    let events = run(
        (false, false),
        &[(100, false, true), (260, false, false)],
        1000,
    );
    assert_eq!(events, [(260, Select)]);
}

#[test]
fn a_hold_of_a_is_up_once_as_soon_as_it_is_long_enough() {
    let events = run(
        (false, false),
        &[(100, true, false), (5000, false, false)],
        6000,
    );
    assert_eq!(
        actions(&events),
        [Up],
        "one hold, one action, and the release adds nothing"
    );
    let at = events[0].0;
    assert!(
        (100 + HOLD_MS..100 + HOLD_MS + 40).contains(&at),
        "fired at {at}, the hold is {HOLD_MS}"
    );
}

#[test]
fn a_press_just_under_the_hold_is_a_click() {
    let events = run(
        (false, false),
        &[(100, true, false), (100 + HOLD_MS - 40, false, false)],
        2000,
    );
    assert_eq!(actions(&events), [Down]);
}

#[test]
fn bounces_are_ignored() {
    // 20 ms is below the shortest press of 40.
    assert!(MIN_PRESS_MS > 20);
    let a = run(
        (false, false),
        &[(100, true, false), (120, false, false)],
        1000,
    );
    let b = run(
        (false, false),
        &[(100, false, true), (120, false, false)],
        1000,
    );
    assert!(a.is_empty() && b.is_empty(), "{a:?} {b:?}");
    // A bouncing contact: down, up, down, up within a few polls, then a real release.
    let events = run(
        (false, false),
        &[
            (100, true, false),
            (120, false, false),
            (140, true, false),
            (160, false, false),
        ],
        1000,
    );
    assert!(events.is_empty(), "{events:?}");
}

#[test]
fn several_clicks_in_a_row_each_count() {
    let events = run(
        (false, false),
        &[
            (100, true, false),
            (200, false, false),
            (400, true, false),
            (500, false, false),
            (700, false, true),
            (800, false, false),
            (1000, true, false),
            (1100, false, false),
        ],
        2000,
    );
    assert_eq!(actions(&events), [Down, Down, Select, Down]);
}

#[test]
fn both_buttons_together_do_nothing_until_both_are_up() {
    // A goes down, B joins, both let go, and then a click of A works again.
    let events = run(
        (false, false),
        &[
            (100, true, false),
            (160, true, true),
            (400, false, true),
            (600, false, false),
            (800, true, false),
            (900, false, false),
        ],
        2000,
    );
    assert_eq!(events, [(900, Down)], "only the click after both were up");
    // Both held for a long time still fires no hold.
    let events = run(
        (false, false),
        &[(100, true, true), (4000, false, false)],
        5000,
    );
    assert!(events.is_empty(), "{events:?}");
}

#[test]
fn a_button_held_while_the_board_starts_does_nothing_until_it_is_let_go() {
    // A is down at start and let go at 1 s: no click, no hold. Then a click works.
    let events = run(
        (true, false),
        &[
            (1000, false, false),
            (1500, true, false),
            (1600, false, false),
        ],
        3000,
    );
    assert_eq!(events, [(1600, Down)]);
    let events = run(
        (false, true),
        &[
            (800, false, false),
            (900, false, true),
            (1000, false, false),
        ],
        3000,
    );
    assert_eq!(events, [(1000, Select)]);
}

#[test]
fn a_hold_then_a_click_are_two_actions() {
    let events = run(
        (false, false),
        &[
            (100, true, false),
            (1200, false, false),
            (1400, true, false),
            (1500, false, false),
        ],
        3000,
    );
    assert_eq!(actions(&events), [Up, Down]);
}

#[test]
fn a_release_and_a_press_of_the_other_button_in_one_poll_lose_nothing() {
    // A is let go at the same poll B is let go: A's click comes first, B's select on the next poll.
    let events = run(
        (false, false),
        &[(100, true, false), (120, true, true), (200, false, false)],
        1000,
    );
    // Both were down together, so neither counts.
    assert!(events.is_empty(), "{events:?}");
    let events = run(
        (false, false),
        &[(100, true, false), (140, true, false), (200, false, false)],
        1000,
    );
    assert_eq!(actions(&events), [Down]);
}

#[test]
fn a_hold_of_b_is_skip_once_as_soon_as_it_is_long_enough() {
    let events = run(
        (false, false),
        &[(100, false, true), (5000, false, false)],
        6000,
    );
    assert_eq!(
        actions(&events),
        [Skip],
        "one hold, one action, and the release adds no select"
    );
    let at = events[0].0;
    assert!(
        (100 + HOLD_MS..100 + HOLD_MS + 40).contains(&at),
        "fired at {at}, the hold is {HOLD_MS}"
    );
}

#[test]
fn a_press_of_b_just_under_the_hold_is_a_select() {
    let events = run(
        (false, false),
        &[(100, false, true), (100 + HOLD_MS - 40, false, false)],
        2000,
    );
    assert_eq!(actions(&events), [Select]);
}

#[test]
fn a_hold_of_b_then_a_click_of_a_are_two_actions() {
    let events = run(
        (false, false),
        &[
            (100, false, true),
            (1000, false, false),
            (1200, true, false),
            (1300, false, false),
        ],
        2000,
    );
    assert_eq!(actions(&events), [Skip, Down]);
}

#[test]
fn a_hold_of_b_that_a_joins_does_nothing() {
    // A goes down while B is held but before the hold is long enough: both together, ignored.
    let events = run(
        (false, false),
        &[(100, false, true), (400, true, true), (3000, false, false)],
        4000,
    );
    assert_eq!(actions(&events), []);
}
