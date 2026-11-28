//! Blocking HTTP GET. WinHTTP on Windows, nothing elsewhere (tests inject a fake).
use crate::{Error, Result};

/// What the updater needs from the network.
pub trait Http {
    /// GET `url`, feeding the body to `sink` in chunks; `sink` returns false to cancel. The second
    /// callback argument is the total size when the server said it. Redirects are followed.
    fn get(&self, url: &str, sink: &mut dyn FnMut(&[u8], Option<u64>) -> bool) -> Result<()>;

    fn get_text(&self, url: &str) -> Result<String> {
        let mut body = Vec::new();
        self.get(url, &mut |chunk, _| {
            body.extend_from_slice(chunk);
            body.len() < 8 << 20
        })?;
        String::from_utf8(body).map_err(|_| Error::Remote("not text".into()))
    }
}

/// The real client.
pub struct System;

#[cfg(not(windows))]
impl Http for System {
    fn get(&self, _url: &str, _sink: &mut dyn FnMut(&[u8], Option<u64>) -> bool) -> Result<()> {
        Err(Error::Offline("downloads are only built for Windows".into()))
    }
}

#[cfg(windows)]
impl Http for System {
    fn get(&self, url: &str, sink: &mut dyn FnMut(&[u8], Option<u64>) -> bool) -> Result<()> {
        winhttp::get(url, sink)
    }
}

#[cfg(windows)]
mod winhttp {
    use super::*;
    use std::ffi::c_void;
    use windows_sys::Win32::Networking::WinHttp::*;

    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: a handle from WinHttp*, closed once.
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn last(what: &str) -> Error {
        Error::Offline(format!("{what} (error {})", std::io::Error::last_os_error().raw_os_error().unwrap_or(0)))
    }

    /// Splits `https://host[:port]/path?query` into (secure, host, port, path).
    pub(super) fn split(url: &str) -> Option<(bool, &str, u16, &str)> {
        let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
            (true, r)
        } else {
            (false, url.strip_prefix("http://")?)
        };
        let (hostport, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) => (h, p.parse().ok()?),
            None => (hostport, if secure { 443 } else { 80 }),
        };
        Some((secure, host, port, path))
    }

    pub fn get(url: &str, sink: &mut dyn FnMut(&[u8], Option<u64>) -> bool) -> Result<()> {
        let (secure, host, port, path) = split(url).ok_or_else(|| Error::Remote(format!("bad url {url}")))?;
        // SAFETY: plain FFI; every pointer is a valid zero terminated wide string or a live buffer,
        // and every handle is closed by `Handle`.
        unsafe {
            let agent = wide("victory-launcher");
            let session = Handle(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, std::ptr::null(), std::ptr::null(), 0));
            if session.0.is_null() {
                return Err(last("WinHttpOpen"));
            }
            WinHttpSetTimeouts(session.0, 15_000, 15_000, 30_000, 60_000);
            let h = wide(host);
            let conn = Handle(WinHttpConnect(session.0, h.as_ptr(), port, 0));
            if conn.0.is_null() {
                return Err(last("connect"));
            }
            let verb = wide("GET");
            let p = wide(path);
            let flags = if secure { WINHTTP_FLAG_SECURE } else { 0 };
            let req = Handle(WinHttpOpenRequest(conn.0, verb.as_ptr(), p.as_ptr(), std::ptr::null(), std::ptr::null(), std::ptr::null(), flags));
            if req.0.is_null() {
                return Err(last("open request"));
            }
            let hdr = wide("Accept: application/octet-stream, application/json\r\n");
            if WinHttpSendRequest(req.0, hdr.as_ptr(), u32::MAX, std::ptr::null(), 0, 0, 0) == 0 {
                return Err(last("send"));
            }
            if WinHttpReceiveResponse(req.0, std::ptr::null_mut()) == 0 {
                return Err(last("receive"));
            }
            let mut status = 0u32;
            let mut len = 4u32;
            WinHttpQueryHeaders(req.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, std::ptr::null(), &mut status as *mut u32 as *mut c_void, &mut len, std::ptr::null_mut());
            if status != 200 {
                return Err(Error::Remote(format!("HTTP {status}")));
            }
            let mut total = 0u64;
            let mut buf = [0u16; 32];
            let mut blen = std::mem::size_of_val(&buf) as u32;
            if WinHttpQueryHeaders(req.0, WINHTTP_QUERY_CONTENT_LENGTH, std::ptr::null(), buf.as_mut_ptr() as *mut c_void, &mut blen, std::ptr::null_mut()) != 0 {
                total = String::from_utf16_lossy(&buf[..(blen / 2) as usize]).trim().parse().unwrap_or(0);
            }
            let total = (total > 0).then_some(total);
            let mut chunk = vec![0u8; 64 * 1024];
            loop {
                let mut got = 0u32;
                if WinHttpReadData(req.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut got) == 0 {
                    return Err(last("read"));
                }
                if got == 0 {
                    return Ok(());
                }
                if !sink(&chunk[..got as usize], total) {
                    return Err(Error::Cancelled);
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn splits_urls() {
            assert_eq!(super::split("https://api.github.com/repos/x/y?z=1"), Some((true, "api.github.com", 443, "/repos/x/y?z=1")));
            assert_eq!(super::split("http://localhost:8080"), Some((false, "localhost", 8080, "/")));
            assert_eq!(super::split("ftp://x"), None);
        }
    }
}
