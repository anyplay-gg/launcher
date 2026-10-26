use anyps5_core::captures::{Filter, Kind};

use super::layout::{Layout, Metrics};
use super::*;

const M: Metrics = Metrics {
    header: 50.0,
    tile_row: 100.0,
    row_gap: 10.0,
    section_gap: 20.0,
};

struct Rig {
    screen: CapturesScreen,
    layout: Layout,
    flat: Vec<usize>,
    kinds: Vec<Kind>,
    button: Option<ButtonKind>,
}

impl Rig {
    /// Two groups of five and two, four to a row; the second group's last capture is a clip.
    fn new() -> Rig {
        let mut screen = CapturesScreen::default();
        screen.open();
        let kinds = vec![
            Kind::Screenshot,
            Kind::Screenshot,
            Kind::Clip,
            Kind::Screenshot,
            Kind::Screenshot,
            Kind::Screenshot,
            Kind::Clip,
        ];
        Rig {
            screen,
            layout: Layout::build(&[5, 2], 4, M),
            flat: (10..17).collect(),
            kinds,
            button: None,
        }
    }

    fn press(&mut self, command: Command) -> Outcome {
        let nav = Nav {
            layout: &self.layout,
            flat: &self.flat,
            kinds: &self.kinds,
            button: self.button,
        };
        self.screen.handle(command, &nav)
    }

    fn pointer(&mut self, pressed: paint::Pressed) -> Outcome {
        let nav = Nav {
            layout: &self.layout,
            flat: &self.flat,
            kinds: &self.kinds,
            button: self.button,
        };
        self.screen.pressed(pressed, &nav)
    }

    fn presses(&mut self, commands: &[Command]) -> Vec<Outcome> {
        commands.iter().map(|c| self.press(*c)).collect()
    }
}

#[test]
fn opening_puts_the_focus_on_the_first_capture() {
    let rig = Rig::new();
    assert_eq!(rig.screen.zone(), Zone::Grid(0));
    assert_eq!(rig.screen.mode(), Mode::Browse);
    assert_eq!(rig.screen.filter(), Filter::All);
}

#[test]
fn the_grid_moves_in_two_dimensions_with_a_sound_for_each_step() {
    let mut rig = Rig::new();
    let out = rig.press(Command::Right);
    assert_eq!(
        (rig.screen.zone(), out.cues),
        (Zone::Grid(1), vec![Cue::Move])
    );
    rig.press(Command::Down);
    assert_eq!(
        rig.screen.zone(),
        Zone::Grid(4),
        "one row down; the row has one tile"
    );
    rig.press(Command::Down);
    assert_eq!(rig.screen.zone(), Zone::Grid(5), "into the next group");
    assert_eq!(rig.press(Command::Down).cues, vec![Cue::Bump]);
}

#[test]
fn the_edges_bump_and_up_from_the_top_row_reaches_the_filters() {
    let mut rig = Rig::new();
    let out = rig.press(Command::Left);
    assert_eq!(
        (rig.screen.zone(), out.cues),
        (Zone::Grid(0), vec![Cue::Bump])
    );
    let out = rig.press(Command::Up);
    assert_eq!(
        (rig.screen.zone(), out.cues),
        (Zone::Filter(0), vec![Cue::Move])
    );
    // Down from the filters lands on the first capture again.
    rig.press(Command::Down);
    assert_eq!(rig.screen.zone(), Zone::Grid(0));
    // The last tile has nowhere to go to the right or down.
    rig.screen.zone = Zone::Grid(6);
    assert_eq!(rig.press(Command::Right).cues, vec![Cue::Bump]);
    assert_eq!(rig.press(Command::Down).cues, vec![Cue::Bump]);
}

#[test]
fn the_filter_chips_choose_with_select_and_stop_at_both_ends() {
    let mut rig = Rig::new();
    rig.screen.zone = Zone::Filter(0);
    assert_eq!(rig.press(Command::Left).cues, vec![Cue::Bump]);
    rig.press(Command::Right);
    rig.press(Command::Right);
    assert_eq!(rig.screen.zone(), Zone::Filter(2));
    assert_eq!(rig.press(Command::Right).cues, vec![Cue::Bump]);
    assert_eq!(rig.screen.filter(), Filter::All, "moving does not choose");
    let out = rig.press(Command::Select);
    assert_eq!(rig.screen.filter(), Filter::Clips);
    assert_eq!(out.cues, vec![Cue::Select]);
    // Choosing the chosen one again is quiet.
    assert!(rig.press(Command::Select).cues.is_empty());
}

#[test]
fn select_opens_the_view_and_left_and_right_step_through_the_captures() {
    let mut rig = Rig::new();
    rig.screen.zone = Zone::Grid(3);
    let out = rig.press(Command::Select);
    assert_eq!(out.cues, vec![Cue::Open]);
    assert_eq!(
        rig.screen.mode(),
        Mode::View {
            place: 3,
            focus: ViewFocus::Media
        }
    );
    rig.press(Command::Right);
    rig.press(Command::Right);
    assert_eq!(
        rig.screen.mode(),
        Mode::View {
            place: 5,
            focus: ViewFocus::Media
        },
        "across the group border"
    );
    assert_eq!(rig.screen.zone(), Zone::Grid(5), "the grid follows");
    rig.screen.mode = Mode::View {
        place: 6,
        focus: ViewFocus::Media,
    };
    assert_eq!(rig.press(Command::Right).cues, vec![Cue::Bump], "the end");
    rig.press(Command::Back);
    assert_eq!(rig.screen.mode(), Mode::Browse);
    assert_eq!(
        rig.screen.zone(),
        Zone::Grid(6),
        "back lands on the capture last seen"
    );
    assert!(
        rig.screen.is_active(),
        "back from the view does not close the area"
    );
}

#[test]
fn a_clip_plays_on_select_and_a_screenshot_goes_to_its_buttons() {
    let mut rig = Rig::new();
    rig.screen.mode = Mode::View {
        place: 2,
        focus: ViewFocus::Media,
    };
    let out = rig.press(Command::Select);
    assert_eq!(out.actions, vec![CapturesAction::OpenInPlayer(12)]);
    rig.screen.mode = Mode::View {
        place: 0,
        focus: ViewFocus::Media,
    };
    let out = rig.press(Command::Select);
    assert!(out.actions.is_empty());
    assert_eq!(
        rig.screen.mode(),
        Mode::View {
            place: 0,
            focus: ViewFocus::Action(0)
        }
    );
}

#[test]
fn the_buttons_of_the_view_differ_by_kind_and_stop_at_the_ends() {
    let mut rig = Rig::new();
    // A screenshot: Copy Image, Show in Folder, Delete.
    rig.screen.mode = Mode::View {
        place: 0,
        focus: ViewFocus::Action(0),
    };
    assert_eq!(
        rig.press(Command::Select).actions,
        vec![CapturesAction::CopyImage(10)]
    );
    rig.press(Command::Right);
    assert_eq!(
        rig.press(Command::Select).actions,
        vec![CapturesAction::ShowInFolder(10)]
    );
    rig.press(Command::Right);
    assert_eq!(rig.press(Command::Right).cues, vec![Cue::Bump]);
    // A clip: Open in Player first.
    rig.screen.mode = Mode::View {
        place: 2,
        focus: ViewFocus::Action(0),
    };
    assert_eq!(
        rig.press(Command::Select).actions,
        vec![CapturesAction::OpenInPlayer(12)]
    );
    assert_eq!(
        rig.press(Command::Up).cues,
        vec![Cue::Move],
        "back up to the picture"
    );
}

#[test]
fn delete_asks_first_opens_on_cancel_and_deletes_only_on_a_second_choice() {
    let mut rig = Rig::new();
    rig.screen.mode = Mode::View {
        place: 4,
        focus: ViewFocus::Action(2),
    };
    let out = rig.press(Command::Select);
    assert!(out.actions.is_empty(), "nothing is deleted by asking");
    assert_eq!(
        rig.screen.mode(),
        Mode::Confirm {
            place: 4,
            button: 0
        }
    );
    // Select on the default does nothing but return.
    let out = rig.press(Command::Select);
    assert!(out.actions.is_empty());
    assert_eq!(
        rig.screen.mode(),
        Mode::View {
            place: 4,
            focus: ViewFocus::Action(2)
        }
    );
    // Back is Cancel too.
    rig.press(Command::Select);
    rig.press(Command::Back);
    assert!(matches!(rig.screen.mode(), Mode::View { .. }));
    // Moving to Delete and selecting deletes that capture.
    rig.press(Command::Select);
    rig.press(Command::Right);
    assert_eq!(
        rig.screen.mode(),
        Mode::Confirm {
            place: 4,
            button: 1
        }
    );
    let out = rig.press(Command::Select);
    assert_eq!(out.actions, vec![CapturesAction::Delete(14)]);
}

#[test]
fn back_closes_from_browsing_and_says_so() {
    let mut rig = Rig::new();
    let out = rig.press(Command::Back);
    assert_eq!(out.actions, vec![CapturesAction::Close]);
    assert_eq!(out.cues, vec![Cue::Close]);
    assert!(!rig.screen.open && rig.screen.is_active(), "it slides out");
}

#[test]
fn a_state_with_a_button_is_reached_from_the_filters() {
    let mut rig = Rig::new();
    rig.flat.clear();
    rig.kinds.clear();
    rig.layout = Layout::build(&[], 4, M);
    rig.button = Some(ButtonKind::Action(CapturesAction::OpenClipsSettings));
    rig.screen.zone = Zone::Filter(0);
    rig.press(Command::Down);
    assert_eq!(rig.screen.zone(), Zone::Button);
    let out = rig.press(Command::Select);
    assert_eq!(out.actions, vec![CapturesAction::OpenClipsSettings]);
    rig.press(Command::Up);
    assert_eq!(rig.screen.zone(), Zone::Filter(0));
    // Without a button there is nowhere below the filters.
    rig.button = None;
    assert_eq!(rig.press(Command::Down).cues, vec![Cue::Bump]);
}

#[test]
fn show_all_lifts_the_filter() {
    let mut rig = Rig::new();
    rig.screen.set_filter(Filter::Clips);
    rig.button = Some(ButtonKind::ShowAll);
    rig.screen.zone = Zone::Button;
    rig.press(Command::Select);
    assert_eq!(rig.screen.filter(), Filter::All);
    assert_eq!(rig.screen.zone(), Zone::Filter(0));
}

#[test]
fn the_focus_stays_on_something_that_is_there() {
    let mut rig = Rig::new();
    // The capture the view shows was the last; it is deleted.
    rig.screen.mode = Mode::View {
        place: 6,
        focus: ViewFocus::Media,
    };
    rig.screen.zone = Zone::Grid(6);
    rig.screen
        .settle(&[10, 11, 12, 13, 14, 15], Load::Ready, false);
    assert_eq!(
        rig.screen.mode(),
        Mode::View {
            place: 5,
            focus: ViewFocus::Media
        }
    );
    assert_eq!(rig.screen.zone(), Zone::Grid(5));
    // The last one goes: back to browsing, on the filters.
    rig.screen.settle(&[], Load::Ready, false);
    assert_eq!(rig.screen.mode(), Mode::Browse);
    assert_eq!(rig.screen.zone(), Zone::Filter(0));
    // A list that is reloading keeps no tile focus either.
    rig.screen.zone = Zone::Grid(2);
    rig.screen.settle(&[10, 11, 12], Load::Loading, false);
    assert_eq!(rig.screen.zone(), Zone::Filter(0));
}

#[test]
fn a_state_with_a_button_opens_on_it() {
    let mut rig = Rig::new();
    rig.screen.settle(&[], Load::Ready, true);
    assert_eq!(rig.screen.zone(), Zone::Button);
    // Once there, it stays; without a button the chips take the focus.
    rig.screen.settle(&[], Load::Ready, true);
    assert_eq!(rig.screen.zone(), Zone::Button);
    rig.screen.settle(&[], Load::Ready, false);
    assert_eq!(rig.screen.zone(), Zone::Filter(0));
}

#[test]
fn a_press_with_the_pointer_does_what_the_keys_do() {
    let mut rig = Rig::new();
    let out = rig.pointer(paint::Pressed {
        tile: Some(2),
        ..Default::default()
    });
    assert_eq!(out.cues, vec![Cue::Open]);
    assert_eq!(
        rig.screen.mode(),
        Mode::View {
            place: 2,
            focus: ViewFocus::Media
        }
    );
    let out = rig.pointer(paint::Pressed {
        play: true,
        ..Default::default()
    });
    assert_eq!(out.actions, vec![CapturesAction::OpenInPlayer(12)]);
    // Clicking Delete asks; clicking its Delete answers.
    rig.pointer(paint::Pressed {
        action: Some(2),
        ..Default::default()
    });
    assert_eq!(
        rig.screen.mode(),
        Mode::Confirm {
            place: 2,
            button: 0
        }
    );
    let out = rig.pointer(paint::Pressed {
        confirm: Some(1),
        ..Default::default()
    });
    assert_eq!(out.actions, vec![CapturesAction::Delete(12)]);
}

#[test]
fn lengths_read_as_minutes_and_seconds() {
    use std::time::Duration;
    assert_eq!(duration_text(Duration::from_secs(5)), "0:05");
    assert_eq!(duration_text(Duration::from_millis(30_500)), "0:31");
    assert_eq!(duration_text(Duration::from_secs(125)), "2:05");
    assert_eq!(duration_text(Duration::from_secs(3_725)), "1:02:05");
}
