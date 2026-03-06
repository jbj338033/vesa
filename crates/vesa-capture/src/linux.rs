use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use evdev::{Device, EventType, InputEventKind, RelativeAxisType};
use nix::poll::{PollFd, PollFlags, PollTimeout};
use tokio::sync::mpsc;
use vesa_event::{Axis, ButtonState, InputEvent, KeyState};

use crate::{CaptureError, InputCapture};

fn current_time_ms() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u32
}

fn is_keyboard_or_mouse(device: &Device) -> bool {
    let types = device.supported_events();
    types.contains(EventType::KEY) || types.contains(EventType::RELATIVE)
}

fn find_input_devices() -> Result<Vec<Device>, CaptureError> {
    let devices: Vec<Device> = evdev::enumerate()
        .map(|(_, d)| d)
        .filter(is_keyboard_or_mouse)
        .collect();

    if devices.is_empty() {
        return Err(CaptureError::NoInputDevices);
    }

    Ok(devices)
}

fn convert_evdev_event(ev: &evdev::InputEvent) -> Option<InputEvent> {
    let time = current_time_ms();

    match ev.kind() {
        InputEventKind::RelAxis(axis) => match axis {
            RelativeAxisType::REL_X => Some(InputEvent::PointerMotion {
                time,
                dx: f64::from(ev.value()),
                dy: 0.0,
            }),
            RelativeAxisType::REL_Y => Some(InputEvent::PointerMotion {
                time,
                dx: 0.0,
                dy: f64::from(ev.value()),
            }),
            RelativeAxisType::REL_WHEEL => Some(InputEvent::PointerAxis {
                time,
                axis: Axis::Vertical,
                value: f64::from(ev.value()),
            }),
            RelativeAxisType::REL_HWHEEL => Some(InputEvent::PointerAxis {
                time,
                axis: Axis::Horizontal,
                value: f64::from(ev.value()),
            }),
            _ => None,
        },
        InputEventKind::Key(key) => {
            let code = key.0;
            // Mouse buttons: BTN_LEFT(0x110)..BTN_TASK(0x117)
            if (0x110..=0x117).contains(&code) {
                let state = if ev.value() == 0 {
                    ButtonState::Release
                } else {
                    ButtonState::Press
                };
                Some(InputEvent::PointerButton {
                    time,
                    button: u32::from(code),
                    state,
                })
            } else {
                let state = match ev.value() {
                    0 => KeyState::Release,
                    1 => KeyState::Press,
                    2 => KeyState::Repeat,
                    _ => return None,
                };
                Some(InputEvent::KeyboardKey {
                    time,
                    key: u32::from(code),
                    state,
                })
            }
        }
        _ => None,
    }
}

pub struct LinuxCapture {
    capturing: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl LinuxCapture {
    pub fn new() -> Result<Self, CaptureError> {
        Ok(Self {
            capturing: Arc::new(AtomicBool::new(false)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        })
    }
}

impl InputCapture for LinuxCapture {
    fn start(&mut self) -> Result<mpsc::Receiver<InputEvent>, CaptureError> {
        if self.thread_handle.is_some() {
            return Err(CaptureError::AlreadyRunning);
        }

        let mut devices = find_input_devices()?;

        let (tx, rx) = mpsc::channel(256);
        let stop_flag = self.stop_flag.clone();
        let capturing = self.capturing.clone();

        stop_flag.store(false, Ordering::Relaxed);

        let thread = std::thread::spawn(move || {
            tracing::info!("Linux input capture started with {} devices", devices.len());

            let mut grabbed = false;

            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }

                // Build poll fds
                let mut poll_fds: Vec<PollFd> = devices
                    .iter()
                    .map(|d| {
                        PollFd::new(
                            unsafe { std::os::fd::BorrowedFd::borrow_raw(d.as_raw_fd()) },
                            PollFlags::POLLIN,
                        )
                    })
                    .collect();

                match nix::poll::poll(&mut poll_fds, PollTimeout::from(100u16)) {
                    Ok(0) => {
                        // timeout — still check grab state below
                    }
                    Err(_) => continue,
                    Ok(_) => {
                        for (i, pfd) in poll_fds.iter().enumerate() {
                            if let Some(revents) = pfd.revents() {
                                if revents.contains(PollFlags::POLLIN) {
                                    if let Ok(events) = devices[i].fetch_events() {
                                        for ev in events {
                                            if let Some(input_event) = convert_evdev_event(&ev) {
                                                let _ = tx.try_send(input_event);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Grab/ungrab only on state change
                let should_grab = capturing.load(Ordering::Relaxed);
                if should_grab && !grabbed {
                    for device in &mut devices {
                        let _ = device.grab();
                    }
                    grabbed = true;
                } else if !should_grab && grabbed {
                    for device in &mut devices {
                        let _ = device.ungrab();
                    }
                    grabbed = false;
                }
            }

            // Ensure ungrab on exit
            if grabbed {
                for device in &mut devices {
                    let _ = device.ungrab();
                }
            }

            tracing::info!("Linux input capture stopped");
        });

        self.thread_handle = Some(thread);
        Ok(rx)
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        let thread = self.thread_handle.take().ok_or(CaptureError::NotRunning)?;
        self.stop_flag.store(true, Ordering::Relaxed);
        let _ = thread.join();
        self.capturing.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn set_capturing(&mut self, capturing: bool) {
        self.capturing.store(capturing, Ordering::Relaxed);
    }
}
