use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Position {
    Left,
    Right,
    Top,
    Bottom,
}

impl Position {
    pub fn to_byte(self) -> u8 {
        match self {
            Position::Left => 0,
            Position::Right => 1,
            Position::Top => 2,
            Position::Bottom => 3,
        }
    }

    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Position::Left),
            1 => Some(Position::Right),
            2 => Some(Position::Top),
            3 => Some(Position::Bottom),
            _ => None,
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Position::Left => Position::Right,
            Position::Right => Position::Left,
            Position::Top => Position::Bottom,
            Position::Bottom => Position::Top,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ButtonState {
    Press,
    Release,
}

impl ButtonState {
    pub fn to_u32(self) -> u32 {
        match self {
            ButtonState::Press => 1,
            ButtonState::Release => 0,
        }
    }

    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            1 => Some(ButtonState::Press),
            0 => Some(ButtonState::Release),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyState {
    Press,
    Release,
    Repeat,
}

impl KeyState {
    pub fn to_u8(self) -> u8 {
        match self {
            KeyState::Press => 1,
            KeyState::Release => 0,
            KeyState::Repeat => 2,
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(KeyState::Press),
            0 => Some(KeyState::Release),
            2 => Some(KeyState::Repeat),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    Vertical,
    Horizontal,
}

impl Axis {
    pub fn to_u8(self) -> u8 {
        match self {
            Axis::Vertical => 0,
            Axis::Horizontal => 1,
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Axis::Vertical),
            1 => Some(Axis::Horizontal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    PointerMotion {
        time: u32,
        dx: f64,
        dy: f64,
    },
    PointerButton {
        time: u32,
        button: u32,
        state: ButtonState,
    },
    PointerAxis {
        time: u32,
        axis: Axis,
        value: f64,
    },
    KeyboardKey {
        time: u32,
        key: u32,
        state: KeyState,
    },
    KeyboardModifiers {
        depressed: u32,
        latched: u32,
        locked: u32,
        group: u32,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ScreenBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_byte_roundtrip() {
        for pos in [Position::Left, Position::Right, Position::Top, Position::Bottom] {
            assert_eq!(Position::from_byte(pos.to_byte()), Some(pos));
        }
    }

    #[test]
    fn position_from_invalid_byte() {
        assert_eq!(Position::from_byte(4), None);
        assert_eq!(Position::from_byte(255), None);
    }

    #[test]
    fn position_opposite() {
        assert_eq!(Position::Left.opposite(), Position::Right);
        assert_eq!(Position::Right.opposite(), Position::Left);
        assert_eq!(Position::Top.opposite(), Position::Bottom);
        assert_eq!(Position::Bottom.opposite(), Position::Top);
    }

    #[test]
    fn position_opposite_is_involution() {
        for pos in [Position::Left, Position::Right, Position::Top, Position::Bottom] {
            assert_eq!(pos.opposite().opposite(), pos);
        }
    }

    #[test]
    fn button_state_roundtrip() {
        assert_eq!(ButtonState::from_u32(ButtonState::Press.to_u32()), Some(ButtonState::Press));
        assert_eq!(ButtonState::from_u32(ButtonState::Release.to_u32()), Some(ButtonState::Release));
    }

    #[test]
    fn button_state_from_invalid() {
        assert_eq!(ButtonState::from_u32(2), None);
        assert_eq!(ButtonState::from_u32(u32::MAX), None);
    }

    #[test]
    fn key_state_roundtrip() {
        for state in [KeyState::Press, KeyState::Release, KeyState::Repeat] {
            assert_eq!(KeyState::from_u8(state.to_u8()), Some(state));
        }
    }

    #[test]
    fn key_state_from_invalid() {
        assert_eq!(KeyState::from_u8(3), None);
        assert_eq!(KeyState::from_u8(255), None);
    }

    #[test]
    fn axis_roundtrip() {
        assert_eq!(Axis::from_u8(Axis::Vertical.to_u8()), Some(Axis::Vertical));
        assert_eq!(Axis::from_u8(Axis::Horizontal.to_u8()), Some(Axis::Horizontal));
    }

    #[test]
    fn axis_from_invalid() {
        assert_eq!(Axis::from_u8(2), None);
    }

    #[test]
    fn position_serde_roundtrip() {
        let json = serde_json::to_string(&Position::Left).unwrap();
        let back: Position = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Position::Left);
    }

    #[test]
    fn screen_bounds_serde() {
        let bounds = ScreenBounds { x: -100, y: 0, width: 1920, height: 1080 };
        let json = serde_json::to_string(&bounds).unwrap();
        let back: ScreenBounds = serde_json::from_str(&json).unwrap();
        assert_eq!(back.x, bounds.x);
        assert_eq!(back.width, bounds.width);
    }
}
