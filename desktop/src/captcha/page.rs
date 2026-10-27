use cumulus_core::{Error, Result};
use tauri::{
    Url,
    http::{Request, Response},
};

// Official SDK and its service/CDN families only. This policy is not applied
// to the main app window. No music API, arbitrary HTTP or app IPC access.
const CSP: &str = "default-src 'none'; script-src 'self' 'unsafe-eval' https://cstaticdun.126.net https://cstatic.dun.163yun.com https://acstatic-dun.126.net https://c.dun.163.com https://c.dun.163yun.com https://ac.dun.163.com https://ac.dun.163yun.com https://*.nstool.netease.com; style-src 'self' 'unsafe-inline' https://cstaticdun.126.net https://cstatic.dun.163yun.com; img-src data: https://*.nosdn.127.net https://*.nosdn.126.net https://cstaticdun.126.net https://*.dun.163.com https://*.dun.163yun.com; connect-src https://*.dun.163.com https://*.dun.163yun.com; font-src data: https://cstaticdun.126.net; base-uri 'none'; form-action 'none'; frame-src 'none'; object-src 'none'";

pub(super) fn local(url: &Url) -> bool {
    ((url.scheme() == "cumulus-captcha" && url.host_str() == Some("localhost"))
        || (matches!(url.scheme(), "http" | "https")
            && url.host_str() == Some("cumulus-captcha.localhost")))
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}
pub(super) fn result(url: &Url) -> Result<String> {
    if !local(url)
        || url.path() != "/result"
        || url.fragment().is_some()
        || url.as_str().len() > 32768
    {
        return Err(Error::State("CAPTCHA unavailable"));
    }
    let pairs = url.query_pairs().collect::<Vec<_>>();
    match pairs.as_slice() {
        [(key, value)]
            if key == "validate"
                && !value.is_empty()
                && value.len() <= 8192
                && !value.chars().any(char::is_control) =>
        {
            Ok(value.to_string())
        }
        [(key, value)] if key == "status" && value == "cancelled" => Err(Error::Cancelled),
        _ => Err(Error::State("CAPTCHA unavailable")),
    }
}
pub(super) fn response(label: &str, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let valid = label.starts_with("captcha-")
        && request.method() == "GET"
        && request.uri().query().is_none();
    let (status, mime, body) = match (valid, request.uri().path()) {
        (true, "/") => (
            200,
            "text/html; charset=utf-8",
            include_bytes!("page.html").as_slice(),
        ),
        (true, "/captcha.js") => (
            200,
            "application/javascript; charset=utf-8",
            include_bytes!("page.js").as_slice(),
        ),
        _ => (404, "text/plain", b"Unavailable".as_slice()),
    };
    Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Content-Security-Policy", CSP)
        .header("Cache-Control", "no-store")
        .header("Referrer-Policy", "no-referrer")
        .header("X-Content-Type-Options", "nosniff")
        .body(body.to_vec())
        .expect("fixed CAPTCHA response")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_is_exact_bounded_and_never_a_business_request() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert_eq!(
            result(&u("cumulus-captcha://localhost/result?validate=a%2Bb")).unwrap(),
            "a+b"
        );
        for s in [
            "https://cumulus-captcha.evil.com/result?validate=x",
            "cumulus-captcha://localhost/result?validate=x&validate=y",
            "cumulus-captcha://localhost/result?validate=%0A",
            "cumulus-captcha://localhost/result?validate=",
            "cumulus-captcha://localhost/result?validate=x#extra",
        ] {
            assert!(result(&u(s)).is_err());
        }
        assert_eq!(
            response("main", &Request::builder().uri("/").body(vec![]).unwrap()).status(),
            404
        );
    }
}
