use evdev::uinput::VirtualDeviceBuilder;
use evdev::{AttributeSet, EventType, InputEvent as EvdevInputEvent, Key, RelativeAxisType};
use vesa_event::{Axis, ButtonState, InputEvent, KeyState};

use crate::{EmulateError, InputEmulate};

pub struct LinuxEmulate {
    virtual_device: evdev::uinput::VirtualDevice,
}

impl LinuxEmulate {
    pub fn new() -> Result<Self, EmulateError> {
        let mut keys = AttributeSet::<Key>::new();
        keys.insert(Key::BTN_LEFT);
        keys.insert(Key::BTN_RIGHT);
        keys.insert(Key::BTN_MIDDLE);
        for code in 1u16..=248 {
            keys.insert(Key::new(code));
        }

        let mut rel_axes = AttributeSet::<RelativeAxisType>::new();
        rel_axes.insert(RelativeAxisType::REL_X);
        rel_axes.insert(RelativeAxisType::REL_Y);
        rel_axes.insert(RelativeAxisType::REL_WHEEL);
        rel_axes.insert(RelativeAxisType::REL_HWHEEL);

        let device = VirtualDeviceBuilder::new()
            .map_err(|e| EmulateError::VirtualDeviceCreation(e.to_string()))?
            .name("vesa-virtual-input")
            .with_keys(&keys)
            .map_err(|e| EmulateError::VirtualDeviceCreation(e.to_string()))?
            .with_relative_axes(&rel_axes)
            .map_err(|e| EmulateError::VirtualDeviceCreation(e.to_string()))?
            .build()
            .map_err(|e| EmulateError::VirtualDeviceCreation(e.to_string()))?;

        Ok(Self {
            virtual_device: device,
        })
    }

    fn emit_events(&mut self, events: &[EvdevInputEvent]) -> Result<(), EmulateError> {
        self.virtual_device
            .emit(events)
            .map_err(|e| EmulateError::VirtualDeviceCreation(e.to_string()))
    }

    fn emit_pointer_motion(&mut self, dx: f64, dy: f64) -> Result<(), EmulateError> {
        let mut events = Vec::with_capacity(3);

        if dx != 0.0 {
            events.push(EvdevInputEvent::new(
                EventType::RELATIVE,
                RelativeAxisType::REL_X.0,
                dx as i32,
            ));
        }
        if dy != 0.0 {
            events.push(EvdevInputEvent::new(
                EventType::RELATIVE,
                RelativeAxisType::REL_Y.0,
                dy as i32,
            ));
        }

        if !events.is_empty() {
            events.push(EvdevInputEvent::new(EventType::SYNCHRONIZATION, 0, 0));
            self.emit_events(&events)?;
        }
        Ok(())
    }

    fn emit_pointer_button(&mut self, button: u32, state: ButtonState) -> Result<(), EmulateError> {
        let value = match state {
            ButtonState::Press => 1,
            ButtonState::Release => 0,
        };

        let events = [
            EvdevInputEvent::new(EventType::KEY, button as u16, value),
            EvdevInputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
        ];

        self.emit_events(&events)
    }

    fn emit_pointer_axis(&mut self, axis: Axis, value: f64) -> Result<(), EmulateError> {
        let axis_type = match axis {
            Axis::Vertical => RelativeAxisType::REL_WHEEL,
            Axis::Horizontal => RelativeAxisType::REL_HWHEEL,
        };

        let events = [
            EvdevInputEvent::new(EventType::RELATIVE, axis_type.0, value as i32),
            EvdevInputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
        ];

        self.emit_events(&events)
    }

    fn emit_keyboard_key(&mut self, key: u32, state: KeyState) -> Result<(), EmulateError> {
        let value = match state {
            KeyState::Release => 0,
            KeyState::Press => 1,
            KeyState::Repeat => 2,
        };

        let events = [
            EvdevInputEvent::new(EventType::KEY, key as u16, value),
            EvdevInputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
        ];

        self.emit_events(&events)
    }
}

impl InputEmulate for LinuxEmulate {
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
        tracing::debug!("LinuxEmulate destroyed");
    }
}
