//! WinHTTP wrapper. Source: `OSTPlatform/Windows/Http.cpp`

use std::ffi::c_void;

#[derive(Clone, Debug)]
pub struct Result {
    pub body: Vec<u8>,
    pub status: u32,
    pub ok: bool,
}

#[derive(Clone, Debug)]
pub struct Request<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub body: Option<&'a [u8]>,
    pub headers: Option<&'a str>,
    pub timeout_resolve_ms: u32,
    pub timeout_connect_ms: u32,
    pub timeout_send_ms: u32,
    pub timeout_recv_ms: u32,
    pub max_body_bytes: u32,
}

impl<'a> Request<'a> {
    pub fn new(method: &'a str, url: &'a str) -> Self {
        Self {
            method,
            url,
            body: None,
            headers: None,
            timeout_resolve_ms: 5000,
            timeout_connect_ms: 5000,
            timeout_send_ms: 10000,
            timeout_recv_ms: 10000,
            max_body_bytes: 256 * 1024,
        }
    }
}

// ─── Windows implementation ─────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen,
        WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData,
        WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_STATUS_CODE,
    };

    // All of these are NULL-pointer constants in the Win32 SDK that
    // windows-sys does not re-export as named items.
    const WINHTTP_NO_PROXY_NAME: *const u16 = std::ptr::null();
    const WINHTTP_NO_PROXY_BYPASS: *const u16 = std::ptr::null();
    const WINHTTP_NO_REFERER: *const u16 = std::ptr::null();
    const WINHTTP_NO_ADDITIONAL_HEADERS: *const u16 = std::ptr::null();
    const WINHTTP_DEFAULT_ACCEPT_TYPES: *const *const u16 = std::ptr::null();
    const WINHTTP_HEADER_NAME_BY_INDEX: *const u16 = std::ptr::null();
    const WINHTTP_NO_HEADER_INDEX: *mut u32 = std::ptr::null_mut();

    const INTERNET_DEFAULT_HTTP_PORT: u16 = 80;
    const INTERNET_DEFAULT_HTTPS_PORT: u16 = 443;

    const WINHTTP_OPTION_DECOMPRESSION: u32 = 118;
    const WINHTTP_DECOMPRESSION_FLAG_GZIP: u32 = 0x00000001;
    const WINHTTP_DECOMPRESSION_FLAG_DEFLATE: u32 = 0x00000002;

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    struct ParsedUrl {
        host: Vec<u16>,
        path: Vec<u16>,
        port: u16,
        tls: bool,
        valid: bool,
    }

    fn parse_url(raw: &str) -> ParsedUrl {
        let mut out = ParsedUrl { host: Vec::new(), path: Vec::new(), port: 0, tls: false, valid: false };
        let (tls, rest) = if let Some(r) = raw.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = raw.strip_prefix("http://") {
            (false, r)
        } else {
            return out;
        };
        out.tls = tls;
        out.port = if tls { INTERNET_DEFAULT_HTTPS_PORT } else { INTERNET_DEFAULT_HTTP_PORT };

        let slash = rest.find('/').unwrap_or(rest.len());
        let host_part = &rest[..slash];
        let path_part = if slash < rest.len() { &rest[slash..] } else { "/" };
        out.path = to_wide(path_part);

        if let Some(colon) = host_part.find(':') {
            out.host = to_wide(&host_part[..colon]);
            if let Ok(p) = host_part[colon + 1..].parse::<u16>() {
                if p != 0 { out.port = p; }
            }
        } else {
            out.host = to_wide(host_part);
        }

        out.valid = !out.host.is_empty();
        out
    }

    pub(super) fn execute(req: &Request<'_>) -> Result {
        let mut r = Result { body: Vec::new(), status: 0, ok: false };

        let pu = parse_url(req.url);
        if !pu.valid {
            crate::ostp_warn!("Invalid URL: {}", req.url);
            return r;
        }

        unsafe {
            let h_session = WinHttpOpen(
                to_wide("OpenSteamTool/1.0").as_ptr(),
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                WINHTTP_NO_PROXY_NAME,
                WINHTTP_NO_PROXY_BYPASS,
                0,
            );
            if h_session.is_null() {
                crate::ostp_warn!("{} - WinHttpOpen failed", req.url);
                return r;
            }

            WinHttpSetTimeouts(
                h_session,
                req.timeout_resolve_ms as i32,
                req.timeout_connect_ms as i32,
                req.timeout_send_ms as i32,
                req.timeout_recv_ms as i32,
            );

            {
                let mut flag: u32 =
                    WINHTTP_DECOMPRESSION_FLAG_GZIP | WINHTTP_DECOMPRESSION_FLAG_DEFLATE;
                let _ = WinHttpSetOption(
                    h_session,
                    WINHTTP_OPTION_DECOMPRESSION,
                    &mut flag as *mut _ as *mut c_void,
                    std::mem::size_of::<u32>() as u32,
                );
            }

            let h_connect = WinHttpConnect(h_session, pu.host.as_ptr(), pu.port, 0);
            if h_connect.is_null() {
                crate::ostp_warn!("{} - WinHttpConnect failed", req.url);
                WinHttpCloseHandle(h_session);
                return r;
            }

            let flags = if pu.tls { WINHTTP_FLAG_SECURE } else { 0 };
            let method_wide = to_wide(req.method);
            let h_request = WinHttpOpenRequest(
                h_connect,
                method_wide.as_ptr(),
                pu.path.as_ptr(),
                std::ptr::null(),
                WINHTTP_NO_REFERER,
                WINHTTP_DEFAULT_ACCEPT_TYPES,
                flags,
            );
            if h_request.is_null() {
                crate::ostp_warn!("{} - WinHttpOpenRequest failed", req.url);
                WinHttpCloseHandle(h_connect);
                WinHttpCloseHandle(h_session);
                return r;
            }

            if let Some(hdr) = req.headers {
                if !hdr.is_empty() {
                    let hdr_wide: Vec<u16> = OsStr::new(hdr).encode_wide().collect();
                    let _ = WinHttpAddRequestHeaders(
                        h_request,
                        hdr_wide.as_ptr(),
                        hdr_wide.len() as u32,
                        0x2000_0000 | 0x1000_0000,
                    );
                }
            }

            let body_ptr = req.body.map(|b| b.as_ptr()).unwrap_or(std::ptr::null());
            let body_len = req.body.map(|b| b.len()).unwrap_or(0) as u32;

            let send_ok = WinHttpSendRequest(
                h_request,
                WINHTTP_NO_ADDITIONAL_HEADERS,
                0,
                body_ptr as *const c_void,
                body_len,
                body_len,
                0,
            );
            if send_ok == 0 {
                crate::ostp_warn!("{} - WinHttpSendRequest failed", req.url);
            } else if WinHttpReceiveResponse(h_request, std::ptr::null_mut()) == 0 {
                crate::ostp_warn!("{} - WinHttpReceiveResponse failed", req.url);
            } else {
                let mut status: u32 = 0;
                let mut sz = std::mem::size_of::<u32>() as u32;
                let _ = WinHttpQueryHeaders(
                    h_request,
                    WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                    WINHTTP_HEADER_NAME_BY_INDEX,
                    &mut status as *mut u32 as *mut c_void,
                    &mut sz,
                    WINHTTP_NO_HEADER_INDEX,
                );
                r.status = status;

                loop {
                    let mut avail: u32 = 0;
                    if WinHttpQueryDataAvailable(h_request, &mut avail) == 0 { break; }
                    if avail == 0 { break; }

                    let off = r.body.len();
                    r.body.resize(off + avail as usize, 0);
                    let mut read: u32 = 0;
                    if WinHttpReadData(
                        h_request,
                        r.body[off..].as_mut_ptr() as *mut c_void,
                        avail,
                        &mut read,
                    ) == 0
                    {
                        r.body.truncate(off);
                        break;
                    }
                    r.body.truncate(off + read as usize);
                    if r.body.len() >= req.max_body_bytes as usize { break; }
                }
                r.ok = true;
            }

            WinHttpCloseHandle(h_request);
            WinHttpCloseHandle(h_connect);
            WinHttpCloseHandle(h_session);
        }

        r
    }
}

// ─── Non-Windows fallback ───────────────────────────────────────────

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub(super) fn execute(_req: &Request<'_>) -> Result {
        Result { body: Vec::new(), status: 0, ok: false }
    }
}

pub fn execute(req: &Request<'_>) -> Result { imp::execute(req) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timeouts_match_cpp() {
        let r = Request::new("GET", "https://example.com");
        assert_eq!(r.timeout_resolve_ms, 5000);
        assert_eq!(r.timeout_connect_ms, 5000);
        assert_eq!(r.timeout_send_ms, 10000);
        assert_eq!(r.timeout_recv_ms, 10000);
        assert_eq!(r.max_body_bytes, 256 * 1024);
    }

    #[cfg(not(windows))]
    #[test]
    fn stub_returns_not_ok() {
        let r = Request::new("GET", "https://example.com");
        let resp = execute(&r);
        assert!(!resp.ok);
        assert_eq!(resp.status, 0);
    }

    #[cfg(windows)]
    #[test]
    fn invalid_url_returns_not_ok() {
        let r = Request::new("GET", "not-a-url");
        let resp = execute(&r);
        assert!(!resp.ok);
    }
}
