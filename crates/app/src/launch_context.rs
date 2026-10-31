//! Whether the binary was launched from an interactive terminal or double-clicked.
//! Windows: GetConsoleProcessList — only this process on the console means Explorer gave
//! us a fresh one. Unix: isatty(stdin).

pub fn is_from_terminal() -> bool {
    #[cfg(windows)]
    {
        let mut buffer = [0u32; 8];
        // SAFETY: the buffer outlives the call and its length is passed.
        let count = unsafe { windows_sys::Win32::System::Console::GetConsoleProcessList(buffer.as_mut_ptr(), buffer.len() as u32) };
        count > 1
    }
    #[cfg(unix)]
    {
        // SAFETY: isatty only inspects the descriptor.
        unsafe { libc::isatty(0) == 1 }
    }
    #[cfg(not(any(windows, unix)))]
    {
        true
    }
}

/// Closes or reopens the console Windows opened for a double-clicked exe. The exe stays a
/// console program (the CLI and a terminal launch need one); while the Professional window is
/// used it detaches from its console, which closes it (conhost or a Windows Terminal tab alike).
/// Showing it again allocates a fresh console. No-op elsewhere.
pub fn set_console_visible(visible: bool) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{AllocConsole, FreeConsole};
        // SAFETY: plain Win32 calls without pointers.
        unsafe {
            if visible {
                if AllocConsole() != 0 {
                    habbo_core::console::reinit_terminal();
                }
            } else {
                FreeConsole();
            }
        }
    }
    #[cfg(not(windows))]
    let _ = visible;
}
