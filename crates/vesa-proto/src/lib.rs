use vesa_event::{Axis, ButtonState, InputEvent, KeyState, Position};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    Enter(Position),
    Leave(f64),
    Ack(u32),
    PointerMotion {
        time: u32,
        dx: f64,
        dy: f64,
    },
    PointerButton {
        time: u32,
        button: u32,
        state: u32,
    },
    PointerAxis {
        time: u32,
        axis: u8,
        value: f64,
    },
    KeyboardKey {
        time: u32,
        key: u32,
        state: u8,
    },
    KeyboardModifiers {
        depressed: u32,
        latched: u32,
        locked: u32,
        group: u32,
    },
    Ping,
    Pong,
    AssignPosition(Position),
}

const TAG_ENTER: u8 = 0x01;
const TAG_LEAVE: u8 = 0x02;
const TAG_ACK: u8 = 0x03;
const TAG_POINTER_MOTION: u8 = 0x04;
const TAG_POINTER_BUTTON: u8 = 0x05;
const TAG_POINTER_AXIS: u8 = 0x06;
const TAG_KEYBOARD_KEY: u8 = 0x07;
const TAG_KEYBOARD_MODIFIERS: u8 = 0x08;
const TAG_PING: u8 = 0x09;
const TAG_PONG: u8 = 0x0A;
const TAG_ASSIGN_POSITION: u8 = 0x0B;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("buffer is empty")]
    Empty,
    #[error("unknown tag: 0x{0:02X}")]
    UnknownTag(u8),
    #[error("expected {expected} bytes, got {got}")]
    TooShort { expected: usize, got: usize },
    #[error("invalid position byte: {0}")]
    InvalidPosition(u8),
}

pub fn encode(msg: &Message) -> Vec<u8> {
    match *msg {
        Message::Enter(pos) => vec![TAG_ENTER, pos.to_byte()],
        Message::Leave(y_ratio) => {
            let mut buf = Vec::with_capacity(9);
            buf.push(TAG_LEAVE);
            buf.extend_from_slice(&y_ratio.to_be_bytes());
            buf
        }
        Message::Ack(seq) => {
            let mut buf = Vec::with_capacity(5);
            buf.push(TAG_ACK);
            buf.extend_from_slice(&seq.to_be_bytes());
            buf
        }
        Message::PointerMotion { time, dx, dy } => {
            let mut buf = Vec::with_capacity(21);
            buf.push(TAG_POINTER_MOTION);
            buf.extend_from_slice(&time.to_be_bytes());
            buf.extend_from_slice(&dx.to_be_bytes());
            buf.extend_from_slice(&dy.to_be_bytes());
            buf
        }
        Message::PointerButton {
            time,
            button,
            state,
        } => {
            let mut buf = Vec::with_capacity(13);
            buf.push(TAG_POINTER_BUTTON);
            buf.extend_from_slice(&time.to_be_bytes());
            buf.extend_from_slice(&button.to_be_bytes());
            buf.extend_from_slice(&state.to_be_bytes());
            buf
        }
        Message::PointerAxis { time, axis, value } => {
            let mut buf = Vec::with_capacity(14);
            buf.push(TAG_POINTER_AXIS);
            buf.extend_from_slice(&time.to_be_bytes());
            buf.push(axis);
            buf.extend_from_slice(&value.to_be_bytes());
            buf
        }
        Message::KeyboardKey { time, key, state } => {
            let mut buf = Vec::with_capacity(10);
            buf.push(TAG_KEYBOARD_KEY);
            buf.extend_from_slice(&time.to_be_bytes());
            buf.extend_from_slice(&key.to_be_bytes());
            buf.push(state);
            buf
        }
        Message::KeyboardModifiers {
            depressed,
            latched,
            locked,
            group,
        } => {
            let mut buf = Vec::with_capacity(17);
            buf.push(TAG_KEYBOARD_MODIFIERS);
            buf.extend_from_slice(&depressed.to_be_bytes());
            buf.extend_from_slice(&latched.to_be_bytes());
            buf.extend_from_slice(&locked.to_be_bytes());
            buf.extend_from_slice(&group.to_be_bytes());
            buf
        }
        Message::Ping => vec![TAG_PING],
        Message::Pong => vec![TAG_PONG],
        Message::AssignPosition(pos) => vec![TAG_ASSIGN_POSITION, pos.to_byte()],
    }
}

fn check_len(buf: &[u8], expected: usize) -> Result<(), DecodeError> {
    if buf.len() < expected {
        return Err(DecodeError::TooShort {
            expected,
            got: buf.len(),
        });
    }
    Ok(())
}

pub fn decode(buf: &[u8]) -> Result<Message, DecodeError> {
    if buf.is_empty() {
        return Err(DecodeError::Empty);
    }

    match buf[0] {
        TAG_ENTER => {
            check_len(buf, 2)?;
            let pos = Position::from_byte(buf[1]).ok_or(DecodeError::InvalidPosition(buf[1]))?;
            Ok(Message::Enter(pos))
        }
        TAG_LEAVE => {
            check_len(buf, 9)?;
            let y_ratio = f64::from_be_bytes(buf[1..9].try_into().unwrap());
            Ok(Message::Leave(y_ratio))
        }
        TAG_ACK => {
            check_len(buf, 5)?;
            let seq = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
            Ok(Message::Ack(seq))
        }
        TAG_POINTER_MOTION => {
            check_len(buf, 21)?;
            let time = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
            let dx = f64::from_be_bytes(buf[5..13].try_into().unwrap());
            let dy = f64::from_be_bytes(buf[13..21].try_into().unwrap());
            Ok(Message::PointerMotion { time, dx, dy })
        }
        TAG_POINTER_BUTTON => {
            check_len(buf, 13)?;
            let time = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
            let button = u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]]);
            let state = u32::from_be_bytes([buf[9], buf[10], buf[11], buf[12]]);
            Ok(Message::PointerButton {
                time,
                button,
                state,
            })
        }
        TAG_POINTER_AXIS => {
            check_len(buf, 14)?;
            let time = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
            let axis = buf[5];
            let value = f64::from_be_bytes(buf[6..14].try_into().unwrap());
            Ok(Message::PointerAxis { time, axis, value })
        }
        TAG_KEYBOARD_KEY => {
            check_len(buf, 10)?;
            let time = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
            let key = u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]]);
            let state = buf[9];
            Ok(Message::KeyboardKey { time, key, state })
        }
        TAG_KEYBOARD_MODIFIERS => {
            check_len(buf, 17)?;
            let depressed = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
            let latched = u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]]);
            let locked = u32::from_be_bytes([buf[9], buf[10], buf[11], buf[12]]);
            let group = u32::from_be_bytes([buf[13], buf[14], buf[15], buf[16]]);
            Ok(Message::KeyboardModifiers {
                depressed,
                latched,
                locked,
                group,
            })
        }
        TAG_PING => Ok(Message::Ping),
        TAG_PONG => Ok(Message::Pong),
        TAG_ASSIGN_POSITION => {
            check_len(buf, 2)?;
            let pos = Position::from_byte(buf[1]).ok_or(DecodeError::InvalidPosition(buf[1]))?;
            Ok(Message::AssignPosition(pos))
        }
        tag => Err(DecodeError::UnknownTag(tag)),
    }
}

impl Message {
    pub fn from_input_event(event: &InputEvent) -> Self {
        match *event {
            InputEvent::PointerMotion { time, dx, dy } => Message::PointerMotion { time, dx, dy },
            InputEvent::PointerButton {
                time,
                button,
                state,
            } => Message::PointerButton {
                time,
                button,
                state: state.to_u32(),
            },
            InputEvent::PointerAxis { time, axis, value } => Message::PointerAxis {
                time,
                axis: axis.to_u8(),
                value,
            },
            InputEvent::KeyboardKey { time, key, state } => Message::KeyboardKey {
                time,
                key,
                state: state.to_u8(),
            },
            InputEvent::KeyboardModifiers {
                depressed,
                latched,
                locked,
                group,
            } => Message::KeyboardModifiers {
                depressed,
                latched,
                locked,
                group,
            },
        }
    }

    pub fn to_input_event(&self) -> Option<InputEvent> {
        match *self {
            Message::PointerMotion { time, dx, dy } => {
                Some(InputEvent::PointerMotion { time, dx, dy })
            }
            Message::PointerButton {
                time,
                button,
                state,
            } => {
                let state = ButtonState::from_u32(state)?;
                Some(InputEvent::PointerButton {
                    time,
                    button,
                    state,
                })
            }
            Message::PointerAxis { time, axis, value } => {
                let axis = Axis::from_u8(axis)?;
                Some(InputEvent::PointerAxis { time, axis, value })
            }
            Message::KeyboardKey { time, key, state } => {
                let state = KeyState::from_u8(state)?;
                Some(InputEvent::KeyboardKey { time, key, state })
            }
            Message::KeyboardModifiers {
                depressed,
                latched,
                locked,
                group,
            } => Some(InputEvent::KeyboardModifiers {
                depressed,
                latched,
                locked,
                group,
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msg: Message) {
        let encoded = encode(&msg);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn enter() {
        roundtrip(Message::Enter(Position::Left));
        roundtrip(Message::Enter(Position::Right));
        roundtrip(Message::Enter(Position::Top));
        roundtrip(Message::Enter(Position::Bottom));
    }

    #[test]
    fn leave() {
        roundtrip(Message::Leave(0.0));
        roundtrip(Message::Leave(0.5));
        roundtrip(Message::Leave(1.0));
    }

    #[test]
    fn ack() {
        roundtrip(Message::Ack(0));
        roundtrip(Message::Ack(42));
        roundtrip(Message::Ack(u32::MAX));
    }

    #[test]
    fn pointer_motion() {
        roundtrip(Message::PointerMotion {
            time: 1000,
            dx: -1.5,
            dy: 2.75,
        });
    }

    #[test]
    fn pointer_button() {
        roundtrip(Message::PointerButton {
            time: 500,
            button: 272,
            state: 1,
        });
    }

    #[test]
    fn pointer_axis() {
        roundtrip(Message::PointerAxis {
            time: 800,
            axis: 0,
            value: -15.0,
        });
    }

    #[test]
    fn keyboard_key() {
        roundtrip(Message::KeyboardKey {
            time: 1200,
            key: 30,
            state: 1,
        });
    }

    #[test]
    fn keyboard_modifiers() {
        roundtrip(Message::KeyboardModifiers {
            depressed: 1,
            latched: 2,
            locked: 3,
            group: 4,
        });
    }

    #[test]
    fn ping_pong() {
        roundtrip(Message::Ping);
        roundtrip(Message::Pong);
    }

    #[test]
    fn assign_position() {
        roundtrip(Message::AssignPosition(Position::Left));
        roundtrip(Message::AssignPosition(Position::Right));
        roundtrip(Message::AssignPosition(Position::Top));
        roundtrip(Message::AssignPosition(Position::Bottom));
    }

    #[test]
    fn decode_empty() {
        assert!(matches!(decode(&[]), Err(DecodeError::Empty)));
    }

    #[test]
    fn decode_unknown_tag() {
        assert!(matches!(
            decode(&[0xFF]),
            Err(DecodeError::UnknownTag(0xFF))
        ));
    }

    #[test]
    fn decode_too_short() {
        assert!(matches!(
            decode(&[TAG_ACK, 0x00]),
            Err(DecodeError::TooShort { .. })
        ));
    }

    #[test]
    fn input_event_roundtrip() {
        let event = InputEvent::PointerMotion {
            time: 100,
            dx: 1.0,
            dy: -1.0,
        };
        let msg = Message::from_input_event(&event);
        let back = msg.to_input_event().unwrap();
        match (event, back) {
            (
                InputEvent::PointerMotion {
                    time: t1,
                    dx: dx1,
                    dy: dy1,
                },
                InputEvent::PointerMotion {
                    time: t2,
                    dx: dx2,
                    dy: dy2,
                },
            ) => {
                assert_eq!(t1, t2);
                assert_eq!(dx1, dx2);
                assert_eq!(dy1, dy2);
            }
            _ => panic!("mismatch"),
        }
    }

    #[test]
    fn non_input_messages_return_none() {
        assert!(Message::Enter(Position::Left).to_input_event().is_none());
        assert!(Message::Leave(0.5).to_input_event().is_none());
        assert!(Message::Ack(0).to_input_event().is_none());
        assert!(Message::Ping.to_input_event().is_none());
        assert!(Message::Pong.to_input_event().is_none());
        assert!(
            Message::AssignPosition(Position::Right)
                .to_input_event()
                .is_none()
        );
    }

    #[test]
    fn encoded_sizes() {
        assert_eq!(encode(&Message::Enter(Position::Left)).len(), 2);
        assert_eq!(encode(&Message::Leave(0.0)).len(), 9);
        assert_eq!(encode(&Message::Ack(0)).len(), 5);
        assert_eq!(
            encode(&Message::PointerMotion {
                time: 0,
                dx: 0.0,
                dy: 0.0
            })
            .len(),
            21
        );
        assert_eq!(
            encode(&Message::PointerButton {
                time: 0,
                button: 0,
                state: 0
            })
            .len(),
            13
        );
        assert_eq!(
            encode(&Message::PointerAxis {
                time: 0,
                axis: 0,
                value: 0.0
            })
            .len(),
            14
        );
        assert_eq!(
            encode(&Message::KeyboardKey {
                time: 0,
                key: 0,
                state: 0
            })
            .len(),
            10
        );
        assert_eq!(
            encode(&Message::KeyboardModifiers {
                depressed: 0,
                latched: 0,
                locked: 0,
                group: 0
            })
            .len(),
            17
        );
        assert_eq!(encode(&Message::Ping).len(), 1);
        assert_eq!(encode(&Message::Pong).len(), 1);
        assert_eq!(encode(&Message::AssignPosition(Position::Right)).len(), 2);
    }
}
