use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::mpsc;
use vesa_event::{Axis, ButtonState, InputEvent, KeyState};
use windows::Win32::Foundation::POINT;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetCursorPos, GetMessageW, GetSystemMetrics, KBDLLHOOKSTRUCT, MSG,
    MSLLHOOKSTRUCT, PostThreadMessageW, SM_CXSCREEN, SM_CYSCREEN, SetCursorPos, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use crate::{CaptureError, InputCapture};

const WM_MOUSEHWHEEL: u32 = 0x020E;

fn current_time_ms() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u32
}

fn vk_to_evdev(vk: u32) -> u32 {
    match vk {
        0x1B => 1,
        0x70..=0x87 => {
            let f = vk - 0x70;
            match f {
                0..=9 => 59 + f,
                10 => 87,
                11 => 88,
                _ => 183 + (f - 12),
            }
        }
        0x30..=0x39 => {
            let n = vk - 0x30;
            if n == 0 { 11 } else { n + 1 }
        }
        0x41..=0x5A => {
            let letter = vk - 0x41;
            const MAP: [u32; 26] = [
                30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22,
                47, 17, 45, 21, 44,
            ];
            MAP[letter as usize]
        }
        0x08 => 14,
        0x09 => 15,
        0x0D => 28,
        0x20 => 57,
        0x10 => 42,
        0xA0 => 42,
        0xA1 => 54,
        0x11 => 29,
        0xA2 => 29,
        0xA3 => 97,
        0x12 => 56,
        0xA4 => 56,
        0xA5 => 100,
        0x5B => 125,
        0x5C => 126,
        0x14 => 58,
        0x25 => 105,
        0x26 => 103,
        0x27 => 106,
        0x28 => 108,
        0x2D => 110,
        0x2E => 111,
        0x24 => 102,
        0x23 => 107,
        0x21 => 104,
        0x22 => 109,
        0x90 => 69,
        0x91 => 70,
        0xBA => 39,
        0xBB => 13,
        0xBC => 51,
        0xBD => 12,
        0xBE => 52,
        0xBF => 53,
        0xC0 => 41,
        0xDB => 26,
        0xDC => 43,
        0xDD => 27,
        0xDE => 40,
        _ => vk,
    }
}

thread_local! {
    static HOOK_SENDER: std::cell::RefCell<Option<mpsc::Sender<InputEvent>>> = const { std::cell::RefCell::new(None) };
    static CAPTURING_FLAG: std::cell::RefCell<Option<Arc<AtomicBool>>> = const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn mouse_hook_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        let info = unsafe { &*(l_param.0 as *const MSLLHOOKSTRUCT) };
        let time = current_time_ms();
        let msg = w_param.0 as u32;

        let event = match msg {
            WM_MOUSEMOVE => None,
            WM_LBUTTONDOWN => Some(InputEvent::PointerButton {
                time,
                button: 0x110,
                state: ButtonState::Press,
            }),
            WM_LBUTTONUP => Some(InputEvent::PointerButton {
                time,
                button: 0x110,
                state: ButtonState::Release,
            }),
            WM_RBUTTONDOWN => Some(InputEvent::PointerButton {
                time,
                button: 0x111,
                state: ButtonState::Press,
            }),
            WM_RBUTTONUP => Some(InputEvent::PointerButton {
                time,
                button: 0x111,
                state: ButtonState::Release,
            }),
            WM_MBUTTONDOWN => Some(InputEvent::PointerButton {
                time,
                button: 0x112,
                state: ButtonState::Press,
            }),
            WM_MBUTTONUP => Some(InputEvent::PointerButton {
                time,
                button: 0x112,
                state: ButtonState::Release,
            }),
            WM_XBUTTONDOWN => {
                let xbutton = (info.mouseData >> 16) & 0xFFFF;
                let button = 0x113 + xbutton.saturating_sub(1);
                Some(InputEvent::PointerButton {
                    time,
                    button,
                    state: ButtonState::Press,
                })
            }
            WM_XBUTTONUP => {
                let xbutton = (info.mouseData >> 16) & 0xFFFF;
                let button = 0x113 + xbutton.saturating_sub(1);
                Some(InputEvent::PointerButton {
                    time,
                    button,
                    state: ButtonState::Release,
                })
            }
            WM_MOUSEWHEEL => {
                let delta = (info.mouseData >> 16) as i16;
                Some(InputEvent::PointerAxis {
                    time,
                    axis: Axis::Vertical,
                    value: f64::from(delta) / 120.0,
                })
            }
            WM_MOUSEHWHEEL => {
                let delta = (info.mouseData >> 16) as i16;
                Some(InputEvent::PointerAxis {
                    time,
                    axis: Axis::Horizontal,
                    value: f64::from(delta) / 120.0,
                })
            }
            _ => None,
        };

        if let Some(ev) = event {
            HOOK_SENDER.with(|s| {
                if let Some(sender) = s.borrow().as_ref() {
                    let _ = sender.try_send(ev);
                }
            });
        }

        if msg == WM_MOUSEMOVE {
            thread_local! {
                static LAST_POS: std::cell::Cell<(i32, i32)> = const { std::cell::Cell::new((i32::MIN, i32::MIN)) };
            }
            LAST_POS.with(|last| {
                let (lx, ly) = last.get();
                if lx != i32::MIN {
                    let dx = info.pt.x - lx;
                    let dy = info.pt.y - ly;
                    if dx != 0 || dy != 0 {
                        let ev = InputEvent::PointerMotion {
                            time,
                            dx: f64::from(dx),
                            dy: f64::from(dy),
                        };
                        HOOK_SENDER.with(|s| {
                            if let Some(sender) = s.borrow().as_ref() {
                                let _ = sender.try_send(ev);
                            }
                        });
                    }
                }
                last.set((info.pt.x, info.pt.y));
            });

            CAPTURING_FLAG.with(|f| {
                if let Some(flag) = f.borrow().as_ref() {
                    if flag.load(Ordering::Relaxed) {
                        unsafe {
                            let cx = GetSystemMetrics(SM_CXSCREEN) / 2;
                            let cy = GetSystemMetrics(SM_CYSCREEN) / 2;
                            let _ = SetCursorPos(cx, cy);
                        }
                    }
                }
            });
        }
    }
    unsafe { CallNextHookEx(None, n_code, w_param, l_param) }
}

unsafe extern "system" fn keyboard_hook_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        let info = unsafe { &*(l_param.0 as *const KBDLLHOOKSTRUCT) };
        let time = current_time_ms();
        let msg = w_param.0 as u32;
        let evdev_key = vk_to_evdev(info.vkCode);

        let state = match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => KeyState::Press,
            WM_KEYUP | WM_SYSKEYUP => KeyState::Release,
            _ => KeyState::Press,
        };

        let event = InputEvent::KeyboardKey {
            time,
            key: evdev_key,
            state,
        };

        HOOK_SENDER.with(|s| {
            if let Some(sender) = s.borrow().as_ref() {
                let _ = sender.try_send(event);
            }
        });
    }
    unsafe { CallNextHookEx(None, n_code, w_param, l_param) }
}

pub struct WindowsCapture {
    capturing: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    thread_id: Option<u32>,
}

impl WindowsCapture {
    pub fn new() -> Result<Self, CaptureError> {
        Ok(Self {
            capturing: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
            thread_id: None,
        })
    }
}

impl InputCapture for WindowsCapture {
    fn start(&mut self) -> Result<mpsc::Receiver<InputEvent>, CaptureError> {
        if self.thread_handle.is_some() {
            return Err(CaptureError::AlreadyRunning);
        }

        let (tx, rx) = mpsc::channel(256);
        let capturing = self.capturing.clone();
        let (tid_tx, tid_rx) = std::sync::mpsc::sync_channel::<u32>(1);

        let thread = std::thread::spawn(move || {
            HOOK_SENDER.with(|s| {
                *s.borrow_mut() = Some(tx);
            });
            CAPTURING_FLAG.with(|f| {
                *f.borrow_mut() = Some(capturing);
            });

            let mouse_hook =
                unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), None, 0) };
            let keyboard_hook =
                unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0) };

            if mouse_hook.is_err() || keyboard_hook.is_err() {
                tracing::error!("failed to install Windows hooks");
                return;
            }

            let mouse_hook = mouse_hook.unwrap();
            let keyboard_hook = keyboard_hook.unwrap();

            let tid = unsafe { GetCurrentThreadId() };
            let _ = tid_tx.send(tid);

            tracing::info!("Windows input capture started");

            let mut msg = MSG::default();
            unsafe {
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message == WM_QUIT {
                        break;
                    }
                }
            }

            unsafe {
                let _ = UnhookWindowsHookEx(mouse_hook);
                let _ = UnhookWindowsHookEx(keyboard_hook);
            }

            HOOK_SENDER.with(|s| {
                *s.borrow_mut() = None;
            });

            tracing::info!("Windows input capture stopped");
        });

        let thread_id = tid_rx.recv().map_err(|_| CaptureError::ThreadFailed)?;

        self.thread_handle = Some(thread);
        self.thread_id = Some(thread_id);

        Ok(rx)
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        let thread = self.thread_handle.take().ok_or(CaptureError::NotRunning)?;
        let tid = self.thread_id.take().ok_or(CaptureError::NotRunning)?;

        unsafe {
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }

        let _ = thread.join();
        self.capturing.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn set_capturing(&mut self, capturing: bool) {
        self.capturing.store(capturing, Ordering::Relaxed);
    }

    fn cursor_position(&self) -> (f64, f64) {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        (f64::from(pt.x), f64::from(pt.y))
    }

    fn screen_bounds(&self) -> (f64, f64, f64, f64) {
        let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        (0.0, 0.0, f64::from(w), f64::from(h))
    }

    fn warp_cursor(&mut self, x: f64, y: f64) {
        unsafe {
            let _ = SetCursorPos(x as i32, y as i32);
        }
    }
}
