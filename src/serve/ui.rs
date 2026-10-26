//! The page `pepe serve` serves at `/`: one HTML file, its script and its
//! style, and a few of pepe's faces, all in the binary. It asks for the
//! token, keeps it for the tab (sessionStorage), and calls the API with it
//! in a header like any other client.

use hyper::{Response, StatusCode};

use super::http::{self, Reply};

const INDEX: &str = include_str!("ui/index.html");
const SCRIPT: &str = include_str!("ui/app.js");
const STYLE: &str = include_str!("ui/app.css");

/// Geist and Geist Mono, the front page's type, latin and variable from
/// 400 to 600: the page loads nothing from elsewhere, so they come with it.
/// The SIL Open Font License, Version 1.1 (fonts/OFL.txt).
const GEIST: &[u8] = include_bytes!("ui/fonts/geist.woff2");
const GEIST_MONO: &[u8] = include_bytes!("ui/fonts/geist-mono.woff2");

/// The faces the page shows, by name
const FACES: [(&str, &[u8]); 6] = [
    ("icon", include_bytes!("../../site/img/pepe-icon.png")),
    ("waiting", include_bytes!("../../site/img/pepe-waiting.png")),
    ("proud", include_bytes!("../../site/img/pepe-proud.png")),
    ("worried", include_bytes!("../../site/img/pepe-worried.png")),
    ("on-fire", include_bytes!("../../site/img/pepe-on-fire.png")),
    (
        "sleeping",
        include_bytes!("../../site/img/pepe-sleeping.png"),
    ),
];

/// Scripts and calls to this server alone; the reports it opens bring
/// their own styles and an image, inline. Never in a frame.
const POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
    img-src 'self' data: blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; \
    form-action 'self'";

fn asset(content_type: &str, body: impl Into<bytes::Bytes>) -> Reply {
    // A font never changes within a version; the page's own files may
    let cache = match content_type.starts_with("font/") {
        true => "public, max-age=604800, immutable",
        false => "no-cache",
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .header("content-security-policy", POLICY)
        .header("x-content-type-options", "nosniff")
        .header("referrer-policy", "no-referrer")
        .header("cache-control", cache)
        .body(http::full(body))
        .expect("a valid response")
}

pub fn serve(path: &str) -> Reply {
    match path {
        "/" | "/index.html" => asset("text/html; charset=utf-8", INDEX),
        "/ui/app.js" => asset("text/javascript; charset=utf-8", SCRIPT),
        "/ui/app.css" => asset("text/css; charset=utf-8", STYLE),
        "/ui/fonts/geist.woff2" => asset("font/woff2", GEIST),
        "/ui/fonts/geist-mono.woff2" => asset("font/woff2", GEIST_MONO),
        _ => {
            let face = path
                .strip_prefix("/ui/img/pepe-")
                .and_then(|p| p.strip_suffix(".png"));
            match face.and_then(|f| FACES.iter().find(|(n, _)| *n == f)) {
                Some((_, png)) => asset("image/png", *png),
                None => http::not_found(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_and_its_parts_are_served_held_by_the_policy() {
        for path in [
            "/",
            "/ui/app.js",
            "/ui/app.css",
            "/ui/img/pepe-proud.png",
            "/ui/fonts/geist.woff2",
            "/ui/fonts/geist-mono.woff2",
        ] {
            let reply = serve(path);
            assert_eq!(reply.status(), StatusCode::OK, "{path}");
            let csp = reply.headers()["content-security-policy"].to_str().unwrap();
            assert!(csp.contains("script-src 'self'") && csp.contains("frame-ancestors 'none'"));
        }
        assert_eq!(
            serve("/ui/img/pepe-../../secret.png").status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(serve("/elsewhere").status(), StatusCode::NOT_FOUND);
        // The fonts are the real thing, and the page's style names them
        assert!(GEIST.starts_with(b"wOF2") && GEIST_MONO.starts_with(b"wOF2"));
        assert!(STYLE.contains("/ui/fonts/geist.woff2") && STYLE.contains("/ui/fonts/geist-mono.woff2"));
        assert_eq!(serve("/ui/fonts/geist.woff2").headers()["content-type"], "font/woff2");
    }

    /// Nothing the page shows is made into markup: what a run or a target
    /// says goes in as text
    #[test]
    fn the_script_sets_text_never_markup() {
        assert!(!SCRIPT.contains("innerHTML"));
        assert!(!SCRIPT.contains("outerHTML"));
        assert!(!SCRIPT.contains("insertAdjacentHTML"));
        assert!(!SCRIPT.contains("eval("));
    }
}
