use core_graphics::display::CGDisplay;
use core_graphics::event::{
    CGEvent, CGEventTapLocation, CGEventType, CGMouseButton, EventField, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use foreign_types::ForeignType;
use vesa_event::{Axis, ButtonState, InputEvent, KeyState};

use crate::{EmulateError, InputEmulate};

unsafe extern "C" {
    fn CGEventSourceSetLocalEventsSuppressionInterval(
        source: core_graphics::sys::CGEventSourceRef,
        seconds: f64,
    );
}

pub struct MacOSEmulate {
    source: CGEventSource,
    mouse_x: f64,
    mouse_y: f64,
    buttons_pressed: [bool; 3],
}

unsafe impl Send for MacOSEmulate {}

impl MacOSEmulate {
    pub fn new() -> Result<Self, EmulateError> {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|()| EmulateError::EventSourceCreation)?;

        unsafe {
            CGEventSourceSetLocalEventsSuppressionInterval(source.as_ptr(), 0.05);
        }

        let (mouse_x, mouse_y) = Self::current_mouse_position();

        Ok(Self {
            source,
            mouse_x,
            mouse_y,
            buttons_pressed: [false; 3],
        })
    }

    fn current_mouse_position() -> (f64, f64) {
        let event =
            CGEvent::new(CGEventSource::new(CGEventSourceStateID::CombinedSessionState).unwrap());
        match event {
            Ok(e) => {
                let loc = e.location();
                (loc.x, loc.y)
            }
            Err(()) => (0.0, 0.0),
        }
    }

    fn clamp_mouse_position(&mut self) {
        let display = CGDisplay::main();
        let width = display.pixels_wide() as f64;
        let height = display.pixels_high() as f64;
        self.mouse_x = self.mouse_x.clamp(0.0, width - 1.0);
        self.mouse_y = self.mouse_y.clamp(0.0, height - 1.0);
    }

    fn mouse_point(&self) -> CGPoint {
        CGPoint::new(self.mouse_x, self.mouse_y)
    }

    fn move_type(&self) -> CGEventType {
        if self.buttons_pressed[0] {
            CGEventType::LeftMouseDragged
        } else if self.buttons_pressed[1] {
            CGEventType::RightMouseDragged
        } else if self.buttons_pressed[2] {
            CGEventType::OtherMouseDragged
        } else {
            CGEventType::MouseMoved
        }
    }

    fn emit_pointer_motion(&mut self, dx: f64, dy: f64) -> Result<(), EmulateError> {
        self.mouse_x += dx;
        self.mouse_y += dy;
        self.clamp_mouse_position();

        let move_type = self.move_type();
        let event = CGEvent::new_mouse_event(
            self.source.clone(),
            move_type,
            self.mouse_point(),
            CGMouseButton::Left,
        )
        .map_err(|()| EmulateError::EventCreation("mouse move".into()))?;

        event.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_X, dx as i64);
        event.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y, dy as i64);
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn emit_pointer_button(&mut self, button: u32, state: ButtonState) -> Result<(), EmulateError> {
        let pressed = matches!(state, ButtonState::Press);
        // Linux evdev button codes: BTN_LEFT=0x110, BTN_RIGHT=0x111, BTN_MIDDLE=0x112
        let (event_type, cg_button) = match button {
            0x110 => {
                self.buttons_pressed[0] = pressed;
                let ty = if pressed {
                    CGEventType::LeftMouseDown
                } else {
                    CGEventType::LeftMouseUp
                };
                (ty, CGMouseButton::Left)
            }
            0x111 => {
                self.buttons_pressed[1] = pressed;
                let ty = if pressed {
                    CGEventType::RightMouseDown
                } else {
                    CGEventType::RightMouseUp
                };
                (ty, CGMouseButton::Right)
            }
            0x112 => {
                self.buttons_pressed[2] = pressed;
                let ty = if pressed {
                    CGEventType::OtherMouseDown
                } else {
                    CGEventType::OtherMouseUp
                };
                (ty, CGMouseButton::Center)
            }
            _ => return Ok(()),
        };

        let event = CGEvent::new_mouse_event(
            self.source.clone(),
            event_type,
            self.mouse_point(),
            cg_button,
        )
        .map_err(|()| EmulateError::EventCreation("mouse button".into()))?;

        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn emit_pointer_axis(&mut self, axis: Axis, value: f64) -> Result<(), EmulateError> {
        let (wheel1, wheel2) = match axis {
            Axis::Vertical => (value as i32, 0),
            Axis::Horizontal => (0, value as i32),
        };

        let event = CGEvent::new_scroll_event(
            self.source.clone(),
            ScrollEventUnit::PIXEL,
            2,
            wheel1,
            wheel2,
            0,
        )
        .map_err(|()| EmulateError::EventCreation("scroll".into()))?;

        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn emit_keyboard_key(&mut self, key: u32, state: KeyState) -> Result<(), EmulateError> {
        let keydown = matches!(state, KeyState::Press | KeyState::Repeat);
        let keycode = key as u16;

        let event = CGEvent::new_keyboard_event(self.source.clone(), keycode, keydown)
            .map_err(|()| EmulateError::EventCreation("keyboard".into()))?;

        event.post(CGEventTapLocation::HID);
        Ok(())
    }
}

impl InputEmulate for MacOSEmulate {
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
        tracing::debug!("MacOSEmulate destroyed");
    }

    fn cursor_position(&self) -> (f64, f64) {
        (self.mouse_x, self.mouse_y)
    }

    fn screen_bounds(&self) -> (f64, f64, f64, f64) {
        let display = CGDisplay::main();
        let bounds = display.bounds();
        (
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            bounds.size.height,
        )
    }
}
