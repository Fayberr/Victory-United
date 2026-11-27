//! A global shortcut registered on a Windows message thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    modifiers: u32,
    key: u32,
}

pub fn parse(text: &str) -> anyhow::Result<Option<Key>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let mut parts = text.rsplit('+').map(str::trim);
    let last = parts.next().unwrap_or_default().to_ascii_uppercase();
    let key = if last.len() == 1 && last.as_bytes()[0].is_ascii_alphanumeric() {
        u32::from(last.as_bytes()[0])
    } else if let Some(n) = last
        .strip_prefix('F')
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|n| (1..=24).contains(n))
    {
        0x70 + n - 1
    } else {
        anyhow::bail!("Use a letter, digit or F1 through F24 for the shortcut key.");
    };
    let mut modifiers = 0;
    for part in parts {
        let modifier = match part.to_ascii_lowercase().as_str() {
            "alt" => 1,
            "ctrl" | "control" => 2,
            "shift" => 4,
            "win" | "windows" => 8,
            _ => anyhow::bail!("Use Ctrl, Alt, Shift or Win as shortcut modifiers."),
        };
        anyhow::ensure!(modifiers & modifier == 0, "Each modifier can be used once.");
        modifiers |= modifier;
    }
    anyhow::ensure!(modifiers != 0, "Include at least one shortcut modifier.");
    Ok(Some(Key { modifiers, key }))
}

#[cfg(windows)]
pub struct Hotkey {
    thread_id: u32,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(windows)]
impl Hotkey {
    pub fn register(key: Key, on_press: impl Fn() + Send + 'static) -> anyhow::Result<Self> {
        use windows::Win32::{
            System::Threading::GetCurrentThreadId,
            UI::{
                Input::KeyboardAndMouse::{
                    HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
                },
                WindowsAndMessaging::{GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, WM_HOTKEY},
            },
        };
        let (send, ready) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("victory-overlay-hotkey".into())
            .spawn(move || {
                const ID: i32 = 1;
                let mut msg = MSG::default();
                // SAFETY: this thread owns its message queue and registers one thread-scoped shortcut.
                unsafe {
                    let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                    let result = RegisterHotKey(
                        None,
                        ID,
                        HOT_KEY_MODIFIERS(key.modifiers) | MOD_NOREPEAT,
                        key.key,
                    )
                    .map(|()| GetCurrentThreadId());
                    let registered = result.is_ok();
                    if send.send(result).is_ok() && registered {
                        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                            if msg.message == WM_HOTKEY && msg.wParam.0 == ID as usize {
                                on_press();
                            }
                        }
                    }
                    if registered {
                        let _ = UnregisterHotKey(None, ID);
                    }
                }
            })?;
        let thread_id = ready.recv()??;
        Ok(Self {
            thread_id,
            thread: Some(thread),
        })
    }
}

#[cfg(windows)]
impl Drop for Hotkey {
    fn drop(&mut self) {
        use windows::Win32::{
            Foundation::{LPARAM, WPARAM},
            UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT},
        };
        // SAFETY: the worker created its message queue before reporting its thread ID.
        if unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) }.is_ok()
            && let Some(thread) = self.thread.take()
        {
            let _ = thread.join();
        }
    }
}

#[cfg(not(windows))]
pub struct Hotkey;

#[cfg(not(windows))]
impl Hotkey {
    pub fn register(_key: Key, _on_press: impl Fn() + Send + 'static) -> anyhow::Result<Self> {
        anyhow::bail!("Global shortcuts are available on Windows.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_blank_invalid_and_combined_shortcuts() {
        assert_eq!(parse(" ").unwrap(), None);
        assert_eq!(
            parse("Ctrl + Alt + F10").unwrap(),
            Some(Key {
                modifiers: 3,
                key: 0x79
            })
        );
        assert_eq!(
            parse("shift+a").unwrap(),
            Some(Key {
                modifiers: 4,
                key: b'A' as u32
            })
        );
        assert!(parse("F10").is_err());
        assert!(parse("ctrl+ctrl+8").is_err());
        assert!(parse("win+F25").is_err());
    }
}
