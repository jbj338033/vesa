use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;
use vesa_event::{Axis, ButtonState, InputEvent, KeyState};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN,
    WM_XBUTTONUP,
};

use crate::{CaptureError, InputCapture};

/// WM_MOUSEHWHEEL (horizontal scroll) — 0x020E.
/// Defined here because some versions of the windows crate may not export it.
const WM_MOUSEHWHEEL: u32 = 0x020E;

fn current_time_ms() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u32
}

/// Map Windows virtual key code to evdev key code.
/// This covers common keys; extend as needed.
fn vk_to_evdev(vk: u32) -> u32 {
    match vk {
        0x1B => 1,        // VK_ESCAPE -> KEY_ESC
        0x70..=0x87 => {  // VK_F1..VK_F24
            let f = vk - 0x70; // 0-based F-key index
            match f {
                0..=9 => 59 + f,   // F1(59)..F10(68)
                10 => 87,          // F11
                11 => 88,          // F12
                _ => 183 + (f - 12), // F13+
            }
        }
        0x30..=0x39 => {  // '0'-'9'
            let n = vk - 0x30;
            if n == 0 { 11 } else { n + 1 } // KEY_1=2..KEY_9=10, KEY_0=11
        }
        0x41..=0x5A => {  // 'A'-'Z'
            let letter = vk - 0x41;
            // evdev: A=30,B=48,C=46,D=32,E=18,F=33,G=34,H=35,I=23,J=36,K=37,L=38,
            //        M=50,N=49,O=24,P=25,Q=16,R=19,S=31,T=20,U=22,V=47,W=17,X=45,Y=21,Z=44
            const MAP: [u32; 26] = [
                30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50,
                49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44,
            ];
            MAP[letter as usize]
        }
        0x08 => 14,  // VK_BACK -> KEY_BACKSPACE
        0x09 => 15,  // VK_TAB -> KEY_TAB
        0x0D => 28,  // VK_RETURN -> KEY_ENTER
        0x20 => 57,  // VK_SPACE -> KEY_SPACE
        0x10 => 42,  // VK_SHIFT -> KEY_LEFTSHIFT
        0xA0 => 42,  // VK_LSHIFT -> KEY_LEFTSHIFT
        0xA1 => 54,  // VK_RSHIFT -> KEY_RIGHTSHIFT
        0x11 => 29,  // VK_CONTROL -> KEY_LEFTCTRL
        0xA2 => 29,  // VK_LCONTROL -> KEY_LEFTCTRL
        0xA3 => 97,  // VK_RCONTROL -> KEY_RIGHTCTRL
        0x12 => 56,  // VK_MENU (Alt) -> KEY_LEFTALT
        0xA4 => 56,  // VK_LMENU -> KEY_LEFTALT
        0xA5 => 100, // VK_RMENU -> KEY_RIGHTALT
        0x5B => 125, // VK_LWIN -> KEY_LEFTMETA
        0x5C => 126, // VK_RWIN -> KEY_RIGHTMETA
        0x14 => 58,  // VK_CAPITAL -> KEY_CAPSLOCK
        0x25 => 105, // VK_LEFT -> KEY_LEFT
        0x26 => 103, // VK_UP -> KEY_UP
        0x27 => 106, // VK_RIGHT -> KEY_RIGHT
        0x28 => 108, // VK_DOWN -> KEY_DOWN
        0x2D => 110, // VK_INSERT -> KEY_INSERT
        0x2E => 111, // VK_DELETE -> KEY_DELETE
        0x24 => 102, // VK_HOME -> KEY_HOME
        0x23 => 107, // VK_END -> KEY_END
        0x21 => 104, // VK_PRIOR (PageUp) -> KEY_PAGEUP
        0x22 => 109, // VK_NEXT (PageDown) -> KEY_PAGEDOWN
        0x90 => 69,  // VK_NUMLOCK -> KEY_NUMLOCK
        0x91 => 70,  // VK_SCROLL -> KEY_SCROLLLOCK
        0xBA => 39,  // VK_OEM_1 (;:) -> KEY_SEMICOLON
        0xBB => 13,  // VK_OEM_PLUS (=+) -> KEY_EQUAL
        0xBC => 51,  // VK_OEM_COMMA -> KEY_COMMA
        0xBD => 12,  // VK_OEM_MINUS -> KEY_MINUS
        0xBE => 52,  // VK_OEM_PERIOD -> KEY_DOT
        0xBF => 53,  // VK_OEM_2 (/?) -> KEY_SLASH
        0xC0 => 41,  // VK_OEM_3 (`~) -> KEY_GRAVE
        0xDB => 26,  // VK_OEM_4 ([{) -> KEY_LEFTBRACE
        0xDC => 43,  // VK_OEM_5 (\|) -> KEY_BACKSLASH
        0xDD => 27,  // VK_OEM_6 (]}) -> KEY_RIGHTBRACE
        0xDE => 40,  // VK_OEM_7 ('") -> KEY_APOSTROPHE
        _ => vk,     // passthrough for unmapped keys
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
            WM_MOUSEMOVE => {
                // For low-level hooks, we get absolute coords.
                // We compute delta from last position via a static.
                // However, for KVM use, we send absolute as relative deltas
                // from the hook's raw dx/dy. MSLLHOOKSTRUCT doesn't have dx/dy,
                // so we track last position.
                None // Handled below via last_pos tracking
            }
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

        // Handle mouse move separately with delta tracking
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

            // If capturing, warp cursor to center of primary monitor
            CAPTURING_FLAG.with(|f| {
                if let Some(flag) = f.borrow().as_ref() {
                    if flag.load(Ordering::Relaxed) {
                        // Warp to center of primary monitor (SM_CXSCREEN/SM_CYSCREEN)
                        unsafe {
                            use windows::Win32::UI::WindowsAndMessaging::{
                                GetSystemMetrics, SetCursorPos, SM_CXSCREEN, SM_CYSCREEN,
                            };
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

            let mouse_hook = unsafe {
                SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), None, 0)
            };
            let keyboard_hook = unsafe {
                SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0)
            };

            if mouse_hook.is_err() || keyboard_hook.is_err() {
                tracing::error!("failed to install Windows hooks");
                return;
            }

            let mouse_hook = mouse_hook.unwrap();
            let keyboard_hook = keyboard_hook.unwrap();

            let tid = unsafe { GetCurrentThreadId() };
            let _ = tid_tx.send(tid);

            tracing::info!("Windows input capture started");

            // Message pump — required for low-level hooks
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

        let thread_id = tid_rx
            .recv()
            .map_err(|_| CaptureError::ThreadFailed)?;

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
}
