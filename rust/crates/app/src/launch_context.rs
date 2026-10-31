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
