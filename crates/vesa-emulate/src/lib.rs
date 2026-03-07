use vesa_event::InputEvent;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Debug, thiserror::Error)]
pub enum EmulateError {
    #[error("failed to create event source")]
    EventSourceCreation,
    #[error("failed to create event: {0}")]
    EventCreation(String),
    #[error("virtual device creation failed: {0}")]
    VirtualDeviceCreation(String),
    #[error("SendInput failed: {0}")]
    SendInputFailed(String),
    #[error("platform not supported")]
    Unsupported,
}

pub trait InputEmulate: Send + 'static {
    fn emit(&mut self, event: InputEvent) -> Result<(), EmulateError>;
    fn destroy(&mut self);

    fn cursor_position(&self) -> (f64, f64) {
        (0.0, 0.0)
    }

    fn screen_bounds(&self) -> (f64, f64, f64, f64) {
        (0.0, 0.0, 1920.0, 1080.0)
    }
}

pub fn create_emulate() -> Result<Box<dyn InputEmulate>, EmulateError> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacOSEmulate::new()?))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WindowsEmulate::new()?))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::LinuxEmulate::new()?))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err(EmulateError::Unsupported)
    }
}
