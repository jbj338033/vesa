use tokio::sync::mpsc;
use vesa_event::InputEvent;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod linux;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("accessibility permission not granted")]
    AccessibilityNotGranted,
    #[error("failed to create event tap")]
    EventTapCreationFailed,
    #[error("failed to create run loop source")]
    RunLoopSourceFailed,
    #[error("capture already running")]
    AlreadyRunning,
    #[error("capture not running")]
    NotRunning,
    #[error("hook install failed: {0}")]
    HookInstallFailed(String),
    #[error("device open failed: {0}")]
    DeviceOpenFailed(String),
    #[error("no input devices found")]
    NoInputDevices,
    #[error("worker thread failed")]
    ThreadFailed,
    #[error("platform not supported")]
    PlatformNotSupported,
}

pub trait InputCapture: Send + 'static {
    fn start(&mut self) -> Result<mpsc::Receiver<InputEvent>, CaptureError>;
    fn stop(&mut self) -> Result<(), CaptureError>;
    fn set_capturing(&mut self, capturing: bool);
}

pub fn create_capture() -> Result<Box<dyn InputCapture>, CaptureError> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOSCapture::new()?))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WindowsCapture::new()?))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::LinuxCapture::new()?))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err(CaptureError::PlatformNotSupported)
    }
}
