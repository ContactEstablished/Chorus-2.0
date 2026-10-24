//! Windows' WebSocket implementation; only authenticated IPv4 loopback endpoints are accepted.
use std::{ffi::c_void, sync::{Mutex, atomic::{AtomicPtr, Ordering}}};
use windows::{Win32::Networking::WinHttp::*, core::{PCWSTR, w}};

struct Handle(*mut c_void);
impl Handle {
    fn checked(value: *mut c_void) -> Result<Self, String> {
        if value.is_null() { Err("Could not open the Codex connection.".into()) } else { Ok(Self(value)) }
    }
}
impl Drop for Handle { fn drop(&mut self) { unsafe { let _ = WinHttpCloseHandle(self.0); } } }

pub struct Socket { handle: AtomicPtr<c_void>, send: Mutex<()> }
impl Socket {
    pub fn connect(port: u16, token: &str) -> Result<Self, String> {
        // All handles are owned and closed on failure. Windows validates the Upgrade response.
        unsafe {
            let session = Handle::checked(WinHttpOpen(w!("Chorus/0.1"), WINHTTP_ACCESS_TYPE_NO_PROXY, PCWSTR::null(), PCWSTR::null(), 0))?;
            WinHttpSetTimeouts(session.0, 2000, 2000, 2000, 5000).map_err(|_| "Could not configure the Codex connection.")?;
            let connection = Handle::checked(WinHttpConnect(session.0, w!("127.0.0.1"), port, 0))?;
            let request = Handle::checked(WinHttpOpenRequest(connection.0, w!("GET"), w!("/"), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_OPEN_REQUEST_FLAGS(0)))?;
            WinHttpSetOption(Some(request.0), WINHTTP_OPTION_UPGRADE_TO_WEB_SOCKET, None).map_err(|_| "Could not request the Codex WebSocket.")?;
            let header: Vec<u16> = format!("Authorization: Bearer {token}\r\n").encode_utf16().collect();
            WinHttpSendRequest(request.0, Some(&header), None, 0, 0, 0).map_err(|_| "Codex connection failed.")?;
            WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|_| "Codex connection was not accepted.")?;
            // An idle agent or a human reviewing a question is not a dead connection. Keep the
            // HTTP handshake bounded, then allow the WebSocket receive to wait until close.
            WinHttpSetTimeouts(request.0, 2000, 2000, 2000, -1).map_err(|_| "Could not configure the Codex event stream.")?;
            let socket = Handle::checked(WinHttpWebSocketCompleteUpgrade(request.0, None))?;
            // WebSocket handle remains valid independently of the request/connection handles.
            let handle = socket.0; std::mem::forget(socket);
            Ok(Self { handle: AtomicPtr::new(handle), send: Mutex::new(()) })
        }
    }
    pub fn send(&self, value: &serde_json::Value) -> Result<(), String> {
        let _guard = self.send.lock().expect("socket writer");
        let handle = self.handle.load(Ordering::SeqCst);
        if handle.is_null() { return Err("Codex connection closed.".into()); }
        let body = value.to_string();
        if body.len() > 2 * 1024 * 1024 { return Err("Codex request is too large.".into()); }
        let status = unsafe { WinHttpWebSocketSend(handle, WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE, Some(body.as_bytes())) };
        if status == 0 { Ok(()) } else { Err("Codex connection closed during send; do not automatically retry.".into()) }
    }
    pub fn receive(&self) -> Result<serde_json::Value, String> {
        let mut message = Vec::new();
        loop {
            let handle = self.handle.load(Ordering::SeqCst);
            if handle.is_null() { return Err("Codex connection closed.".into()); }
            let mut buf = [0u8; 16384]; let mut read = 0; let mut kind = WINHTTP_WEB_SOCKET_BUFFER_TYPE::default();
            let status = unsafe { WinHttpWebSocketReceive(handle, buf.as_mut_ptr().cast(), buf.len() as u32, &mut read, &mut kind) };
            if status != 0 || !matches!(kind, WINHTTP_WEB_SOCKET_UTF8_FRAGMENT_BUFFER_TYPE | WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE) {
                return Err(format!("Codex event connection ended (Windows status {status}, frame {}).", kind.0));
            }
            if message.len() + read as usize > 2 * 1024 * 1024 { return Err("Codex event exceeded the size limit.".into()); }
            message.extend_from_slice(&buf[..read as usize]);
            if kind == WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE {
                return serde_json::from_slice(&message).map_err(|_| "Codex sent an invalid event.".into());
            }
        }
    }
    pub fn close(&self) {
        let handle = self.handle.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !handle.is_null() { unsafe { let _ = WinHttpCloseHandle(handle); } }
    }
}
impl Drop for Socket { fn drop(&mut self) { self.close(); } }
