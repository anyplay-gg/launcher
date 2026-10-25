//! The phone's pad packet as an Xbox-360-style gamepad state, the layout every virtual controller
//! backend here speaks (XInput's `XINPUT_GAMEPAD`). Pure: no I/O, the same on every platform.

use crate::remote::protocol::{InputPacket, button};

/// XInput button bits.
pub mod xbutton {
    pub const UP: u16 = 0x0001;
    pub const DOWN: u16 = 0x0002;
    pub const LEFT: u16 = 0x0004;
    pub const RIGHT: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const L3: u16 = 0x0040;
    pub const R3: u16 = 0x0080;
    pub const LB: u16 = 0x0100;
    pub const RB: u16 = 0x0200;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
    pub const X: u16 = 0x4000;
    pub const Y: u16 = 0x8000;
}

/// One gamepad state. Stick Y axes are positive up, as in XInput.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PadState {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
}

/// The buttons that map one to one, from the phone's bit to the XInput bit.
const BUTTONS: [(u32, u16); 14] = [
    (button::UP, xbutton::UP),
    (button::DOWN, xbutton::DOWN),
    (button::LEFT, xbutton::LEFT),
    (button::RIGHT, xbutton::RIGHT),
    (button::OPTIONS, xbutton::START),
    (button::TOUCHPAD, xbutton::BACK),
    (button::L3, xbutton::L3),
    (button::R3, xbutton::R3),
    (button::L1, xbutton::LB),
    (button::R1, xbutton::RB),
    (button::CROSS, xbutton::A),
    (button::CIRCLE, xbutton::B),
    (button::SQUARE, xbutton::X),
    (button::TRIANGLE, xbutton::Y),
];

/// 0..=255 with 128 at the centre to the full `i16` range: 0 is -32768, 128 is 0, 255 is 32767.
pub fn axis(value: u8) -> i16 {
    let v = i32::from(value) - 128;
    // 127 steps up to +32767 and 128 down to -32768.
    let scaled = if v >= 0 {
        v * 32767 / 127
    } else {
        v * 32768 / 128
    };
    scaled.clamp(-32768, 32767) as i16
}

/// Flips an axis so up is positive (`-32768` becomes `32767`, the centre stays 0).
fn flip(value: i16) -> i16 {
    value.saturating_neg()
}

fn trigger(analogue: u8, digital: bool) -> u8 {
    if digital { 255 } else { analogue }
}

/// Maps a phone packet. The phone's Y is 0 at the top, XInput's is positive up. Motion and the
/// touch surface have no place on this pad and are dropped.
pub fn map_packet(packet: &InputPacket) -> PadState {
    let buttons = BUTTONS
        .iter()
        .filter(|(from, _)| packet.buttons & from != 0)
        .fold(0, |bits, (_, to)| bits | to);
    let [lx, ly, rx, ry] = packet.sticks;
    PadState {
        buttons,
        left_trigger: trigger(packet.l2, packet.buttons & button::L2 != 0),
        right_trigger: trigger(packet.r2, packet.buttons & button::R2 != 0),
        lx: axis(lx),
        ly: flip(axis(ly)),
        rx: axis(rx),
        ry: flip(axis(ry)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(buttons: u32) -> InputPacket {
        InputPacket {
            buttons,
            ..InputPacket::neutral(1)
        }
    }

    #[test]
    fn every_button_maps_to_its_own_bit() {
        for (from, to) in BUTTONS {
            let state = map_packet(&with(from));
            assert_eq!(state.buttons, to, "phone bit {from:#x}");
            assert_eq!(state.left_trigger, 0);
            assert_eq!(state.right_trigger, 0);
        }
    }

    #[test]
    fn the_table_matches_the_documented_layout() {
        let table = [
            (button::CROSS, 0x1000),
            (button::CIRCLE, 0x2000),
            (button::SQUARE, 0x4000),
            (button::TRIANGLE, 0x8000),
            (button::L1, 0x0100),
            (button::R1, 0x0200),
            (button::OPTIONS, 0x0010),
            (button::TOUCHPAD, 0x0020),
            (button::L3, 0x0040),
            (button::R3, 0x0080),
            (button::UP, 0x0001),
            (button::DOWN, 0x0002),
            (button::LEFT, 0x0004),
            (button::RIGHT, 0x0008),
        ];
        for (from, to) in table {
            assert_eq!(map_packet(&with(from)).buttons, to);
        }
    }

    #[test]
    fn all_buttons_at_once_and_none() {
        assert_eq!(map_packet(&with(0)), PadState::default());
        let all = map_packet(&with(button::ALL));
        assert_eq!(all.buttons, 0xF3FF);
        assert_eq!((all.left_trigger, all.right_trigger), (255, 255));
    }

    #[test]
    fn digital_triggers_raise_a_lower_analogue_value() {
        let mut packet = with(button::L2);
        packet.l2 = 10;
        packet.r2 = 200;
        let state = map_packet(&packet);
        assert_eq!((state.left_trigger, state.right_trigger), (255, 200));
        packet.buttons = button::R2;
        packet.l2 = 77;
        packet.r2 = 5;
        let state = map_packet(&packet);
        assert_eq!((state.left_trigger, state.right_trigger), (77, 255));
        packet.buttons = 0;
        packet.r2 = 255;
        assert_eq!(map_packet(&packet).right_trigger, 255);
    }

    #[test]
    fn the_axis_has_a_true_centre_and_full_ends() {
        assert_eq!(axis(0), -32768);
        assert_eq!(axis(128), 0);
        assert_eq!(axis(255), 32767);
        assert_eq!(axis(127), -256);
        assert_eq!(axis(129), 258);
        let mut last = i32::MIN;
        for value in 0..=255u8 {
            let now = i32::from(axis(value));
            assert!(now > last, "{value}");
            last = now;
        }
    }

    #[test]
    fn y_is_inverted_and_x_is_not() {
        let mut packet = InputPacket::neutral(1);
        packet.sticks = [0, 0, 255, 255];
        let state = map_packet(&packet);
        // Left stick up-left, right stick down-right.
        assert_eq!((state.lx, state.ly), (-32768, 32767));
        assert_eq!((state.rx, state.ry), (32767, -32767));
        packet.sticks = [128; 4];
        let centre = map_packet(&packet);
        assert_eq!((centre.lx, centre.ly, centre.rx, centre.ry), (0, 0, 0, 0));
    }

    #[test]
    fn the_neutral_packet_is_near_rest() {
        let state = map_packet(&InputPacket::neutral(1));
        assert_eq!(state.buttons, 0);
        assert_eq!((state.lx, state.ly, state.rx, state.ry), (0, 0, 0, 0));
    }
}
