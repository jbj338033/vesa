use vesa_event::{Axis, ButtonState, InputEvent, KeyState};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
};

use crate::{EmulateError, InputEmulate};

fn evdev_to_scancode(evdev: u32) -> u16 {
    evdev as u16
}

pub struct WindowsEmulate {
    mouse_x: i32,
    mouse_y: i32,
    screen_width: i32,
    screen_height: i32,
}

impl WindowsEmulate {
    pub fn new() -> Result<Self, EmulateError> {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };

        Ok(Self {
            mouse_x: pt.x,
            mouse_y: pt.y,
            screen_width: sw,
            screen_height: sh,
        })
    }

    fn send_input(&self, input: &[INPUT]) -> Result<(), EmulateError> {
        let sent = unsafe { SendInput(input, size_of::<INPUT>() as i32) };
        if sent == 0 {
            return Err(EmulateError::SendInputFailed("SendInput returned 0".into()));
        }
        Ok(())
    }

    fn emit_pointer_motion(&mut self, dx: f64, dy: f64) -> Result<(), EmulateError> {
        self.mouse_x += dx as i32;
        self.mouse_y += dy as i32;
        self.mouse_x = self.mouse_x.clamp(0, self.screen_width - 1);
        self.mouse_y = self.mouse_y.clamp(0, self.screen_height - 1);

        let abs_x = (self.mouse_x * 65535) / self.screen_width.max(1);
        let abs_y = (self.mouse_y * 65535) / self.screen_height.max(1);

        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: abs_x,
                    dy: abs_y,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        self.send_input(&[input])
    }

    fn emit_pointer_button(&mut self, button: u32, state: ButtonState) -> Result<(), EmulateError> {
        let pressed = matches!(state, ButtonState::Press);
        let flags = match button {
            0x110 => {
                if pressed {
                    MOUSEEVENTF_LEFTDOWN
                } else {
                    MOUSEEVENTF_LEFTUP
                }
            }
            0x111 => {
                if pressed {
                    MOUSEEVENTF_RIGHTDOWN
                } else {
                    MOUSEEVENTF_RIGHTUP
                }
            }
            0x112 => {
                if pressed {
                    MOUSEEVENTF_MIDDLEDOWN
                } else {
                    MOUSEEVENTF_MIDDLEUP
                }
            }
            _ => return Ok(()),
        };

        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        self.send_input(&[input])
    }

    fn emit_pointer_axis(&mut self, axis: Axis, value: f64) -> Result<(), EmulateError> {
        let (flags, data) = match axis {
            Axis::Vertical => (MOUSEEVENTF_WHEEL, (value * 120.0) as i32),
            Axis::Horizontal => (MOUSEEVENTF_HWHEEL, (value * 120.0) as i32),
        };

        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: data as u32,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        self.send_input(&[input])
    }

    fn emit_keyboard_key(&mut self, key: u32, state: KeyState) -> Result<(), EmulateError> {
        let scan = evdev_to_scancode(key);
        let keyup = matches!(state, KeyState::Release);

        let mut flags = KEYEVENTF_SCANCODE;
        if keyup {
            flags |= KEYEVENTF_KEYUP;
        }

        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: Default::default(),
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        self.send_input(&[input])
    }
}

impl InputEmulate for WindowsEmulate {
    fn cursor_position(&self) -> (f64, f64) {
        (f64::from(self.mouse_x), f64::from(self.mouse_y))
    }

    fn screen_bounds(&self) -> (f64, f64, f64, f64) {
        (0.0, 0.0, f64::from(self.screen_width), f64::from(self.screen_height))
    }

    fn emit(&mut self, event: InputEvent) -> Result<(), EmulateError> {
        match event {
            InputEvent::PointerMotion { dx, dy, .. } => self.emit_pointer_motion(dx, dy),
            InputEvent::PointerButton { button, state, .. } => {
                self.emit_pointer_button(button, state)
            }
            InputEvent::PointerAxis { axis, value, .. } => self.emit_pointer_axis(axis, value),
            InputEvent::KeyboardKey { key, state, .. } => self.emit_keyboard_key(key, state),
            InputEvent::KeyboardModifiers { .. } => Ok(()),
        }
    }

    fn destroy(&mut self) {
        tracing::debug!("WindowsEmulate destroyed");
    }
}
