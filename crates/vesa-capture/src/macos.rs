use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use core_foundation::base::TCFType;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop, CFRunLoopRef};
use core_graphics::display::CGDisplay;
use core_graphics::event::{
    CGEvent, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy,
    CGEventType, CallbackResult, EventField,
};
use core_graphics::geometry::CGPoint;
use tokio::sync::mpsc;
use vesa_event::{Axis, ButtonState, InputEvent, KeyState};

use crate::{CaptureError, InputCapture};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: core_foundation::base::CFTypeRef) -> bool;
}

fn check_accessibility() -> bool {
    unsafe {
        use core_foundation::boolean::CFBoolean;
        use core_foundation::dictionary::CFDictionary;
        use core_foundation::string::CFString;

        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let value = CFBoolean::true_value();
        let options =
            CFDictionary::from_CFType_pairs(&[(key, value.as_CFType())]);
        AXIsProcessTrustedWithOptions(options.as_CFTypeRef())
    }
}

fn current_time_ms() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u32
}

fn convert_event(event_type: CGEventType, event: &CGEvent) -> Option<InputEvent> {
    let time = current_time_ms();

    match event_type {
        CGEventType::MouseMoved
        | CGEventType::LeftMouseDragged
        | CGEventType::RightMouseDragged
        | CGEventType::OtherMouseDragged => {
            let dx = event.get_double_value_field(EventField::MOUSE_EVENT_DELTA_X);
            let dy = event.get_double_value_field(EventField::MOUSE_EVENT_DELTA_Y);
            Some(InputEvent::PointerMotion { time, dx, dy })
        }
        CGEventType::LeftMouseDown => Some(InputEvent::PointerButton {
            time,
            button: 0x110, // BTN_LEFT
            state: ButtonState::Press,
        }),
        CGEventType::LeftMouseUp => Some(InputEvent::PointerButton {
            time,
            button: 0x110,
            state: ButtonState::Release,
        }),
        CGEventType::RightMouseDown => Some(InputEvent::PointerButton {
            time,
            button: 0x111, // BTN_RIGHT
            state: ButtonState::Press,
        }),
        CGEventType::RightMouseUp => Some(InputEvent::PointerButton {
            time,
            button: 0x111,
            state: ButtonState::Release,
        }),
        CGEventType::OtherMouseDown => {
            let btn = event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER) as u32;
            let button = match btn {
                2 => 0x112, // BTN_MIDDLE
                n => 0x110 + n,
            };
            Some(InputEvent::PointerButton {
                time,
                button,
                state: ButtonState::Press,
            })
        }
        CGEventType::OtherMouseUp => {
            let btn = event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER) as u32;
            let button = match btn {
                2 => 0x112,
                n => 0x110 + n,
            };
            Some(InputEvent::PointerButton {
                time,
                button,
                state: ButtonState::Release,
            })
        }
        CGEventType::ScrollWheel => {
            let v = event.get_double_value_field(
                EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1,
            );
            let h = event.get_double_value_field(
                EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_2,
            );

            if v.abs() >= h.abs() {
                Some(InputEvent::PointerAxis {
                    time,
                    axis: Axis::Vertical,
                    value: v,
                })
            } else {
                Some(InputEvent::PointerAxis {
                    time,
                    axis: Axis::Horizontal,
                    value: h,
                })
            }
        }
        CGEventType::KeyDown => {
            let keycode =
                event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u32;
            let autorepeat =
                event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT);
            let state = if autorepeat != 0 {
                KeyState::Repeat
            } else {
                KeyState::Press
            };
            Some(InputEvent::KeyboardKey {
                time,
                key: keycode,
                state,
            })
        }
        CGEventType::KeyUp => {
            let keycode =
                event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u32;
            Some(InputEvent::KeyboardKey {
                time,
                key: keycode,
                state: KeyState::Release,
            })
        }
        CGEventType::FlagsChanged => {
            let flags = event.get_flags();
            let bits = flags.bits();
            let depressed = (bits & 0x00FF_0000) as u32;
            let latched = 0u32;
            let locked = if bits & 0x0001_0000 != 0 { 1 } else { 0 };
            let group = 0u32;
            Some(InputEvent::KeyboardModifiers {
                depressed,
                latched,
                locked,
                group,
            })
        }
        _ => None,
    }
}

struct SendPtr(CFRunLoopRef);
unsafe impl Send for SendPtr {}

struct RunLoopHandle {
    run_loop_ref: CFRunLoopRef,
    thread: Option<std::thread::JoinHandle<()>>,
}

unsafe impl Send for RunLoopHandle {}

pub struct MacOSCapture {
    capturing: Arc<AtomicBool>,
    cursor_x: Arc<AtomicU64>,
    cursor_y: Arc<AtomicU64>,
    run_loop_handle: Option<RunLoopHandle>,
}

impl MacOSCapture {
    pub fn new() -> Result<Self, CaptureError> {
        if !check_accessibility() {
            return Err(CaptureError::AccessibilityNotGranted);
        }

        Ok(Self {
            capturing: Arc::new(AtomicBool::new(false)),
            cursor_x: Arc::new(AtomicU64::new(0)),
            cursor_y: Arc::new(AtomicU64::new(0)),
            run_loop_handle: None,
        })
    }
}

impl InputCapture for MacOSCapture {
    fn start(&mut self) -> Result<mpsc::Receiver<InputEvent>, CaptureError> {
        if self.run_loop_handle.is_some() {
            return Err(CaptureError::AlreadyRunning);
        }

        let (tx, rx) = mpsc::channel(256);
        let capturing = self.capturing.clone();
        let cursor_x = self.cursor_x.clone();
        let cursor_y = self.cursor_y.clone();
        let (rl_tx, rl_rx) = std::sync::mpsc::sync_channel::<SendPtr>(1);

        let thread = std::thread::spawn(move || {
            let events_of_interest = vec![
                CGEventType::MouseMoved,
                CGEventType::LeftMouseDown,
                CGEventType::LeftMouseUp,
                CGEventType::RightMouseDown,
                CGEventType::RightMouseUp,
                CGEventType::OtherMouseDown,
                CGEventType::OtherMouseUp,
                CGEventType::LeftMouseDragged,
                CGEventType::RightMouseDragged,
                CGEventType::OtherMouseDragged,
                CGEventType::ScrollWheel,
                CGEventType::KeyDown,
                CGEventType::KeyUp,
                CGEventType::FlagsChanged,
            ];

            let tap = match core_graphics::event::CGEventTap::new(
                CGEventTapLocation::Session,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                events_of_interest,
                move |_proxy: CGEventTapProxy, etype: CGEventType, event: &CGEvent| {
                    // Track cursor position for edge detection
                    let loc = event.location();
                    cursor_x.store(loc.x.to_bits(), Ordering::Relaxed);
                    cursor_y.store(loc.y.to_bits(), Ordering::Relaxed);

                    if let Some(input_event) = convert_event(etype, event) {
                        let _ = tx.try_send(input_event);
                    }

                    if capturing.load(Ordering::Relaxed) {
                        CallbackResult::Drop
                    } else {
                        CallbackResult::Keep
                    }
                },
            ) {
                Ok(tap) => tap,
                Err(()) => {
                    tracing::error!("failed to create CGEventTap");
                    return;
                }
            };

            let loop_source = match tap.mach_port().create_runloop_source(0) {
                Ok(source) => source,
                Err(()) => {
                    tracing::error!("failed to create CFRunLoop source");
                    return;
                }
            };

            let current_loop = CFRunLoop::get_current();
            unsafe {
                current_loop.add_source(&loop_source, kCFRunLoopCommonModes);
            }
            tap.enable();

            let _ = rl_tx.send(SendPtr(current_loop.as_concrete_TypeRef()));

            tracing::info!("macOS input capture started");
            unsafe { core_foundation::runloop::CFRunLoopRun() };
            tracing::info!("macOS input capture stopped");
        });

        let run_loop = rl_rx
            .recv()
            .map_err(|_| CaptureError::EventTapCreationFailed)?
            .0;

        self.run_loop_handle = Some(RunLoopHandle {
            run_loop_ref: run_loop,
            thread: Some(thread),
        });

        Ok(rx)
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        let handle = self
            .run_loop_handle
            .take()
            .ok_or(CaptureError::NotRunning)?;

        unsafe {
            core_foundation::runloop::CFRunLoopStop(handle.run_loop_ref);
        }

        if let Some(thread) = handle.thread {
            let _ = thread.join();
        }

        self.capturing.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn set_capturing(&mut self, capturing: bool) {
        let display = CGDisplay::main();
        if capturing {
            // Warp cursor to center BEFORE setting flag, so the warp event
            // itself isn't suppressed and the cursor actually moves away from the edge.
            let bounds = display.bounds();
            let center = CGPoint::new(
                bounds.origin.x + bounds.size.width / 2.0,
                bounds.origin.y + bounds.size.height / 2.0,
            );
            let _ = CGDisplay::warp_mouse_cursor_position(center);

            self.capturing.store(true, Ordering::Relaxed);
            let _ = CGDisplay::associate_mouse_and_mouse_cursor_position(false);
            let _ = display.hide_cursor();
            tracing::debug!("cursor warped to center, hidden, mouse disassociated");
        } else {
            self.capturing.store(false, Ordering::Relaxed);
            let _ = CGDisplay::associate_mouse_and_mouse_cursor_position(true);
            let _ = display.show_cursor();
            tracing::debug!("cursor shown, mouse reassociated");
        }
    }

    fn cursor_position(&self) -> (f64, f64) {
        let x = f64::from_bits(self.cursor_x.load(Ordering::Relaxed));
        let y = f64::from_bits(self.cursor_y.load(Ordering::Relaxed));
        (x, y)
    }

    fn screen_bounds(&self) -> (f64, f64, f64, f64) {
        let bounds = CGDisplay::main().bounds();
        (
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            bounds.size.height,
        )
    }
}
