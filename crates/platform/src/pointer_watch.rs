//! Temporary, non-consuming click observation for dismissing a floating popup.
use std::{cell::OnceCell, sync::mpsc, thread};
use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    UI::WindowsAndMessaging::*,
};
thread_local! { static SEND: OnceCell<tokio::sync::mpsc::UnboundedSender<(i32, i32)>> = const { OnceCell::new() }; }
#[derive(Debug)]
pub struct Watch {
    id: u32,
    join: Option<thread::JoinHandle<()>>,
}
impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: the message queue belongs to the thread created below.
        unsafe {
            let _ = PostThreadMessageA(self.id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
unsafe extern "system" fn click(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0
        && matches!(
            w.0 as u32,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
        )
    {
        // SAFETY: Windows provides this structure for a nonnegative low-level hook call.
        let info = unsafe { &*(l.0 as *const MSLLHOOKSTRUCT) };
        SEND.with(|slot| {
            if let Some(send) = slot.get() {
                let _ = send.send((info.pt.x, info.pt.y));
            }
        });
    }
    // SAFETY: forward unchanged; never consume clicks or change focus.
    unsafe { CallNextHookEx(None, code, w, l) }
}
pub fn watch(
    send: tokio::sync::mpsc::UnboundedSender<(i32, i32)>,
) -> Result<Watch, crate::PlatformError> {
    let (ready, started) = mpsc::channel();
    let join = thread::Builder::new()
        .name("kw-popup-clicks".into())
        .spawn(move || {
            SEND.with(|slot| {
                let _ = slot.set(send);
            });
            let mut message = MSG::default();
            // SAFETY: valid message storage; create the queue before publishing its id.
            unsafe {
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
            }
            // SAFETY: documented global low-level mouse hook on a dedicated message-loop thread.
            let hook = match unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(click), None, 0) } {
                Ok(hook) => hook,
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                    return;
                }
            };
            // SAFETY: gets the calling thread id.
            let id = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
            if ready.send(Ok(id)).is_ok() {
                // SAFETY: standard message pump with valid storage.
                unsafe {
                    while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
            }
            // SAFETY: remove the hook owned by this thread.
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
        })
        .map_err(|e| crate::PlatformError::Window(e.to_string()))?;
    match started.recv() {
        Ok(Ok(id)) => Ok(Watch {
            id,
            join: Some(join),
        }),
        result => {
            let _ = join.join();
            Err(crate::PlatformError::Window(format!(
                "Popup click observation failed: {result:?}"
            )))
        }
    }
}
