//! Bounded support for the observed Discover challenge, not a JavaScript engine.
use super::http_client::{COOKIE_JAR, USER_AGENT};
use anyhow::{ensure, Context, Result};
use regex::Regex;
use reqwest::cookie::{CookieStore, Jar};
use reqwest::{header, Client, Url};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use std::time::Duration;
use tokio::sync::Mutex;

static SESSION: LazyLock<Mutex<Option<Client>>> = LazyLock::new(|| Mutex::new(None));

pub(super) async fn get(url: &str) -> Result<String> {
    let url = Url::parse(url).context("Invalid Discover URL")?;
    ensure!(
        url.scheme() == "https"
            && url.host_str() == Some("bandcamp.com")
            && matches!(url.path(), "/discover" | "/discover/"),
        "Discover challenge handling is restricted to Bandcamp Discover"
    );
    // Serialize the entire flow so concurrent callers do not race token exchanges.
    let mut session = SESSION.lock().await;
    if session.is_none() {
        *session = Some(
            Client::builder()
                .cookie_provider(COOKIE_JAR.clone())
                .user_agent(USER_AGENT)
                .danger_accept_invalid_certs(super::args::args_no_ssl_verify())
                .gzip(true)
                .timeout(Duration::from_secs(20))
                .connect_timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        );
    }
    fetch(
        session.as_ref().context("Discover client unavailable")?,
        &COOKIE_JAR,
        &url,
    )
    .await
}

fn has_app(html: &str) -> bool {
    let document = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("div#DiscoverApp[data-blob]").unwrap();
    document.select(&selector).next().is_some()
}

async fn page(client: &Client, url: &Url, reload: bool) -> Result<String> {
    let mut request = client.get(url.clone()).header(
        header::ACCEPT,
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
    );
    if reload {
        request = request
            .header("Sec-Fetch-Site", "same-origin")
            .header("Sec-Fetch-Mode", "navigate")
            .header("Sec-Fetch-Dest", "document")
            .header(header::REFERER, url.as_str())
            .header(header::CACHE_CONTROL, "max-age=0")
            .header(header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
            .header("Priority", "u=0, i");
    }
    let response = request.send().await.context("Discover GET failed")?;
    let status = response.status();
    tracing::debug!(%status, reload, "Discover GET");
    ensure!(
        status == reqwest::StatusCode::OK,
        "Discover GET returned HTTP {status}"
    );
    response
        .text()
        .await
        .context("Failed to read Discover HTML")
}

async fn fetch(client: &Client, jar: &Jar, url: &Url) -> Result<String> {
    let html = page(client, url, false).await?;
    if has_app(&html) {
        return Ok(html);
    }
    let script_url = extract_script_url(&html, url)
        .context("Discover has neither app data nor a supported challenge")?;
    tracing::info!("Completing supported Bandcamp Discover challenge");
    let response = client
        .get(script_url.clone())
        .header(header::ACCEPT, "*/*")
        .header(header::REFERER, url.as_str())
        .send()
        .await
        .context("Discover challenge script GET failed")?;
    let status = response.status();
    ensure!(
        status == reqwest::StatusCode::OK,
        "Discover challenge script returned HTTP {status}"
    );
    let init = extract_init(&response.text().await?)?;
    ensure!(
        script_url.path() == format!("{}/script.js", init.prefix),
        "Challenge prefix mismatch"
    );
    ensure!(init.reload, "Unsupported challenge reload mode");
    ensure!(
        init.challenges
            .as_array()
            .is_some_and(|ch| ch.len() == 1 && ch[0]["ty"] == "pat"),
        "Unsupported initial Discover challenge"
    );
    let endpoint = url.join(&format!("{}/fst-post-back", init.prefix))?;
    let initial = json!({"token":init.token,"data":[{"ty":"pat","auth":""}]});
    let response = post_back(client, url, &endpoint, &initial).await?;
    if response["status"] != "success" {
        let body = followup_body(&response)?;
        let response = post_back(client, url, &endpoint, &body).await?;
        ensure!(
            response["status"] == "success",
            "Discover challenge was not accepted; stopping"
        );
    }
    ensure!(
        has_completion_cookie(jar, url),
        "Discover challenge returned no applicable completion cookie"
    );
    let html = page(client, url, true).await?;
    ensure!(
        has_app(&html),
        "Discover app data missing after challenge; stopping without retry"
    );
    tracing::info!("Discover challenge completed and app HTML retrieved");
    Ok(html)
}

async fn post_back(client: &Client, page: &Url, endpoint: &Url, body: &Value) -> Result<Value> {
    let response = client
        .post(endpoint.clone())
        .header(header::ACCEPT, "application/json")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, page.origin().ascii_serialization())
        .header(header::REFERER, page.as_str())
        .body(serde_json::to_vec(body)?)
        .send()
        .await
        .context("Discover challenge post-back failed")?;
    let status = response.status();
    ensure!(
        status == reqwest::StatusCode::OK,
        "Discover challenge post-back returned HTTP {status}"
    );
    serde_json::from_slice(&response.bytes().await?)
        .context("Invalid Discover challenge response JSON")
}

fn has_completion_cookie(jar: &Jar, url: &Url) -> bool {
    jar.cookies(url).is_some_and(|cookies| {
        cookies.to_str().is_ok_and(|cookies| {
            cookies.split(';').any(|cookie| {
                cookie.trim().split_once('=').is_some_and(|(name, value)| {
                    name.starts_with("_fs_ch_cp_") && !value.is_empty()
                })
            })
        })
    })
}

// Intentionally no Debug derive: tokens should not appear in test diagnostics.
struct ChallengeInit {
    challenges: serde_json::Value,
    token: String,
    prefix: String,
    reload: bool,
}

fn extract_script_url(html: &str, page_url: &Url) -> Result<Url> {
    let pattern = Regex::new(
        r#"\bscript\s*\.\s*src\s*=\s*(?:'(?P<single>/_fs-ch-[A-Za-z0-9_-]+/script\.js\?reload=true)'|"(?P<double>/_fs-ch-[A-Za-z0-9_-]+/script\.js\?reload=true)")"#,
    )?;
    let captures = pattern.captures(html).context(
        "Discover body has no supported Fastly script.src assignment (possibly normal HTML)",
    )?;
    let path = captures
        .name("single")
        .or_else(|| captures.name("double"))
        .context("Challenge script path is missing")?;
    Ok(page_url.join(path.as_str())?)
}

fn extract_init(script: &str) -> Result<ChallengeInit> {
    // This is deliberately a parser for the observed JSON-shaped footer, not a JS interpreter.
    let pattern = Regex::new(
        r#"(?s)(?:^|;)\s*init\s*\(\s*(\[.*?\])\s*,\s*("(?:[^"\\]|\\.)*")\s*,\s*("(?:[^"\\]|\\.)*")\s*,\s*(true|false)\s*\)\s*;?\s*\z"#,
    )?;
    let captures = pattern
        .captures(script)
        .context("script.js has no supported trailing init(...) call")?;
    let init = ChallengeInit {
        challenges: serde_json::from_str(&captures[1]).context("Invalid challenge JSON")?,
        token: serde_json::from_str(&captures[2]).context("Invalid token JSON string")?,
        prefix: serde_json::from_str(&captures[3]).context("Invalid prefix JSON string")?,
        reload: captures[4].parse()?,
    };
    ensure!(!init.token.is_empty(), "Challenge token is empty");
    ensure!(
        Regex::new(r"^/_fs-ch-[A-Za-z0-9_-]+$")?.is_match(&init.prefix),
        "Unexpected challenge prefix format"
    );
    Ok(init)
}

fn fixed_client_metrics() -> Value {
    // A synthetic fixture, NOT the result of running the detector in a browser.
    // Align UA-related fields with the HTTP UA; all other values remain fixed.
    let mut client_data = json!({
        "rtt": {"error":"BotdError: navigator.connection is undefined"},
        "windowExternal": {"error":"BotdError: window.external is undefined"},
        "webGL": {"error":"BotdError: WebGLRenderingContext is null"},
        "android":{"value":false}, "browserKind":{"value":"chrome"},
        "browserEngineKind":{"value":"chromium"}, "documentFocus":{"value":false},
        "userAgent":{"value":USER_AGENT},
        "appVersion":{"value":USER_AGENT.strip_prefix("Mozilla/").unwrap()},
        "windowSize":{"value":{"outerWidth":1660,"outerHeight":994,"innerWidth":1660,"innerHeight":445}},
        "pdfViewerEnabled":{"value":true}, "pluginsLength":{"value":5},
        "pluginsArray":{"value":true}, "productSub":{"value":"20030107"},
        "mimeTypesConsistent":{"value":true}, "evalLength":{"value":33},
        "webDriver":{"value":false}, "languages":{"value":[["en-US"],["en-US"]]},
        "notificationPermissions":{"error":"BotdError: window.Notification is undefined"},
        "documentElementKeys":{"value":["lang"]},
        "functionBind":{"value":"function bind() {\n    [native code]\n}"},
        "webglMath":{"value":{"available":false,"claimsHardware":false,"vendorFamily":null,"mathProbed":false,"sampleMatches":null,"renderProbed":false,"renderMatch":null}},
        "iframeSrcdoc":{"value":{"hasSelfGet":false}}
    });
    let mut distinctive = serde_json::Map::new();
    for name in [
        "awesomium",
        "cef",
        "cefsharp",
        "coachjs",
        "fminer",
        "geb",
        "nightmarejs",
        "phantomas",
        "phantomjs",
        "playwright",
        "rhino",
        "selenium",
        "webdriverio",
        "webdriver",
        "headless_chrome",
    ] {
        distinctive.insert(name.into(), json!(false));
    }
    client_data["distinctiveProps"] = json!({"value":distinctive});
    let mut detectors = serde_json::Map::new();
    for name in [
        "AppVersion",
        "DocumentAttributes",
        "ErrorTrace",
        "EvalLengthInconsistency",
        "FunctionBind",
        "LanguagesLengthInconsistency",
        "NotificationPermissions",
        "PluginsArray",
        "PluginsLengthInconsistency",
        "Process",
        "PuppeteerStealth",
        "UserAgent",
        "WebDriver",
        "WebGL",
        "WindowExternal",
        "WindowSize",
        "MimeTypesConsistent",
        "ProductSub",
        "DistinctiveProperties",
        "SpoofedGpuMath",
    ] {
        detectors.insert(format!("detect{name}"), json!(""));
    }
    json!({
        "ty":"clientmetrics", "webdriver":false,
        "bot_detection_result":{"bot_detected":false,"bot_kind":null},
        "browser_metrics":{"client_data":client_data.to_string(),"error_trace":null},
        "evidence":{"datacenter_vgpu":false}, "detector_results":detectors, "v":2
    })
}

fn sha256_hex(input: &[u8]) -> String {
    Sha256::digest(input)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn solve_pow(data: &Value) -> Result<Value> {
    let base = data["base"].as_str().context("Missing PoW base")?;
    let hash = data["hash"].as_str().context("Missing PoW hash")?;
    ensure!(hash.len() == 64, "Expected SHA-256 hash");
    ensure!(
        data["hmac"].is_string() && data["expires"].is_string(),
        "Missing PoW metadata"
    );
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    for first in ALPHABET {
        for second in ALPHABET {
            let answer = format!("{}{}", *first as char, *second as char);
            if sha256_hex(format!("{base}{answer}").as_bytes()) == hash {
                return Ok(json!({"ty":"pow", "base":base, "answer":answer,
                    "hmac":data["hmac"], "expires":data["expires"]}));
            }
        }
    }
    anyhow::bail!("No two-character PoW answer found")
}

fn followup_body(response: &Value) -> Result<Value> {
    let token = response["tok"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("Missing follow-up token")?;
    let challenges = response["ch"]
        .as_array()
        .context("Missing follow-up challenges")?;
    ensure!(
        !challenges.is_empty() && challenges.len() <= 2,
        "Unsupported follow-up challenge count"
    );
    let mut answers = Vec::new();
    for challenge in challenges {
        answers.push(match challenge["ty"].as_str() {
            Some("pow") => solve_pow(&challenge["data"])?,
            Some("clientmetrics") => fixed_client_metrics(),
            _ => anyhow::bail!("Unsupported follow-up challenge; stopping"),
        });
    }
    Ok(json!({"token":token,"data":answers}))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A strict response script also detects accidental retries / extra requests.
    async fn scenario(responses: Vec<(u16, String, &'static str)>) -> Result<String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = Url::parse(&format!("http://{}/discover", listener.local_addr()?))?;
        let server = tokio::spawn(async move {
            for (status, body, cookie) in responses {
                let (mut socket, _) = listener.accept().await?;
                let mut headers = Vec::new();
                while !headers.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    ensure!(socket.read(&mut byte).await? != 0);
                    headers.push(byte[0]);
                    ensure!(headers.len() < 16384);
                }
                let headers = String::from_utf8(headers)?.to_ascii_lowercase();
                if let Some(length) = headers
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: "))
                {
                    let mut body = vec![0; length.parse()?];
                    socket.read_exact(&mut body).await?;
                }
                socket.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nSet-Cookie: {cookie}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
            }
            Ok::<_, anyhow::Error>(())
        });
        let jar = std::sync::Arc::new(Jar::default());
        let client = Client::builder()
            .cookie_provider(jar.clone())
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(2))
            .build()?;
        let result = tokio::time::timeout(Duration::from_secs(5), fetch(&client, &jar, &url)).await;
        if result.as_ref().is_err() {
            server.abort();
        }
        let result = result?;
        server.await??;
        result
    }

    #[tokio::test]
    async fn ordinary_html_is_returned_without_challenge_requests() -> Result<()> {
        let html = "<div\n id='DiscoverApp' data-blob='{&quot;appData&quot;:{}}'></div>";
        assert_eq!(scenario(vec![(200, html.into(), "x=y")]).await?, html);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_status_redirect_unknown_html_and_repeated_challenge() -> Result<()> {
        for status in [302, 403, 500] {
            assert!(scenario(vec![(status, DISCOVER_HTML.into(), "x=y")])
                .await
                .is_err());
        }
        assert!(scenario(vec![(200, "unknown HTML".into(), "x=y")])
            .await
            .is_err());
        for (cookie, reload) in [("x=y", false), ("_fs_ch_cp_test=yes; Path=/", true)] {
            let mut responses = vec![
                (200, DISCOVER_HTML.into(), "x=y"),
                (200, CHALLENGE_JS.into(), "x=y"),
                (200, r#"{"status":"success"}"#.into(), cookie),
            ];
            if reload {
                responses.push((200, DISCOVER_HTML.into(), "x=y"));
            }
            let error = scenario(responses).await.unwrap_err().to_string();
            assert!(error.contains(if reload {
                "without retry"
            } else {
                "completion cookie"
            }));
        }
        Ok(())
    }
    const DISCOVER_HTML: &str = r#"<html><script>
    const script = document.createElement('script');
    script.src = '/_fs-ch-test123/script.js?reload=true';
    document.head.appendChild(script);
</script></html>"#;

    const CHALLENGE_JS: &str = r#""use strict";
(() => { function init() {} })();
;init([{
    "ty": "pat", "data": {}
}], "synthetic-token_123=", "/_fs-ch-test123", true);
"#;

    #[test]
    fn extracts_variable_script_path_and_both_quote_styles() -> Result<()> {
        let page = Url::parse("https://bandcamp.com/discover")?;
        for html in [DISCOVER_HTML.to_owned(), DISCOVER_HTML.replace("'", "\"")] {
            let html = html.replace("test123", "different_ID-42");
            let url = extract_script_url(&html, &page)?;
            assert_eq!(
                url.as_str(),
                "https://bandcamp.com/_fs-ch-different_ID-42/script.js?reload=true"
            );
        }
        Ok(())
    }

    #[test]
    fn rejects_normal_html_and_external_script() -> Result<()> {
        let page = Url::parse("https://bandcamp.com/discover")?;
        assert!(extract_script_url("<div id='DiscoverApp'></div>", &page).is_err());
        assert!(extract_script_url(
            "script.src = 'https://example.com/_fs-ch-test/script.js?reload=true';",
            &page
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn extracts_multiline_init_footer() -> Result<()> {
        let init = extract_init(CHALLENGE_JS)?;
        assert_eq!(init.challenges[0]["ty"], "pat");
        assert_eq!(init.token, "synthetic-token_123=");
        assert_eq!(init.prefix, "/_fs-ch-test123");
        assert!(init.reload);
        let compact = CHALLENGE_JS.replace('\n', "").replace("true);", "false)");
        assert!(!extract_init(&compact)?.reload);
        Ok(())
    }

    #[test]
    fn rejects_missing_init_empty_token_and_invalid_prefix() {
        for script in [
            "function init(a,b,c,d) {}".to_owned(),
            CHALLENGE_JS.replace("synthetic-token_123=", ""),
            CHALLENGE_JS.replace("/_fs-ch-test123", "https://example.com"),
        ] {
            assert!(extract_init(&script).is_err());
        }
    }

    #[tokio::test]
    async fn gets_and_postbacks_preserve_cookies_tokens_and_user_agent() -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let discover = Url::parse(&format!("http://{}/discover", listener.local_addr()?))?;
        let server = tokio::spawn(async move {
            let hash = sha256_hex(b"synthetic-baseaa");
            let followup = json!({"tok":"synthetic-next-token","ch":[
                {"ty":"pow","data":{"base":"synthetic-base","hash":hash,
                    "hmac":"synthetic-hmac","expires":"9999999999"}},
                {"ty":"clientmetrics","data":{}}
            ]})
            .to_string();
            for (index, (method, path, body, cookie)) in [
                (
                    "get",
                    "/discover",
                    DISCOVER_HTML,
                    "first=one; Path=/; HttpOnly",
                ),
                (
                    "get",
                    "/_fs-ch-test123/script.js?reload=true",
                    CHALLENGE_JS,
                    "second=two; Path=/_fs-ch-test123; HttpOnly",
                ),
                (
                    "post",
                    "/_fs-ch-test123/fst-post-back",
                    followup.as_str(),
                    "third=three; Path=/; HttpOnly",
                ),
                (
                    "post",
                    "/_fs-ch-test123/fst-post-back",
                    r#"{"status":"success"}"#,
                    "_fs_ch_cp_synthetic=completed; Path=/; HttpOnly",
                ),
                (
                    "get",
                    "/discover",
                    r#"<div id="DiscoverApp" data-blob="{&quot;appData&quot;:{}}"></div>"#,
                    "done=yes; Path=/; HttpOnly",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let (mut socket, _) = listener.accept().await?;
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    ensure!(socket.read(&mut byte).await? != 0, "Incomplete request");
                    request.push(byte[0]);
                    ensure!(request.len() < 16384, "Request headers too large");
                }
                let request = String::from_utf8(request)?.to_ascii_lowercase();
                ensure!(request.starts_with(&format!("{method} {path} http/1.1\r\n")));
                ensure!(request.contains(&format!(
                    "\r\nuser-agent: {}\r\n",
                    USER_AGENT.to_ascii_lowercase()
                )));
                if path != "/discover" {
                    let cookies = request
                        .lines()
                        .find(|line| line.starts_with("cookie:"))
                        .context("Missing cookies")?;
                    ensure!(cookies.contains("first=one"));
                    if method == "post" {
                        ensure!(cookies.contains("second=two"));
                    }
                    if index == 3 {
                        ensure!(cookies.contains("third=three"));
                    }
                }
                if index == 4 {
                    let cookies = request
                        .lines()
                        .find(|line| line.starts_with("cookie:"))
                        .context("Missing reload cookies")?;
                    ensure!(cookies.contains("_fs_ch_cp_synthetic=completed"));
                    ensure!(cookies.contains("first=one"));
                    ensure!(!cookies.contains("second=two"));
                    ensure!(request.contains("\r\nsec-fetch-mode: navigate\r\n"));
                }
                if method == "post" {
                    let length: usize = request
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .context("Missing body length")?
                        .parse()?;
                    let mut body = vec![0; length];
                    socket.read_exact(&mut body).await?;
                    let body: Value = serde_json::from_slice(&body)?;
                    if index == 2 {
                        ensure!(
                            body == json!({"token":"synthetic-token_123=","data":[{"ty":"pat","auth":""}]})
                        );
                    } else {
                        ensure!(body["token"] == "synthetic-next-token");
                        ensure!(body["data"][0]["answer"] == "aa");
                        ensure!(body["data"][1] == fixed_client_metrics());
                    }
                }
                let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nSet-Cookie: {cookie}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
                socket.write_all(response.as_bytes()).await?;
            }
            Ok::<_, anyhow::Error>(())
        });
        let jar = std::sync::Arc::new(Jar::default());
        let client = Client::builder()
            .cookie_provider(jar.clone())
            .user_agent(USER_AGENT)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let result =
            tokio::time::timeout(Duration::from_secs(5), fetch(&client, &jar, &discover)).await;
        let html = result??;
        server.await??;
        assert!(has_app(&html));
        assert!(has_completion_cookie(&jar, &discover));
        Ok(())
    }

    #[test]
    fn builds_followup_with_new_token_and_computed_answer() -> Result<()> {
        let response = json!({"tok":"synthetic-next-token","ch":[
            {"ty":"pow","data":{"base":"dWfT4Jj8vnVelZPI",
                "hash":"418b3c0cafb4dcd7d3d031a847457928fd2949fab7f9bdc6324929cdf47aa92d",
                "hmac":"synthetic-hmac","expires":"1794970933"}},
            {"ty":"clientmetrics","data":{}}
        ]});
        let body = followup_body(&response)?;
        assert_eq!(body["token"], "synthetic-next-token");
        assert_eq!(body["data"][0]["answer"], "Ji");
        assert_eq!(body["data"][0]["hmac"], "synthetic-hmac");
        assert_eq!(body["data"][0]["expires"], "1794970933");
        let metrics = &body["data"][1];
        let client_data: Value =
            serde_json::from_str(metrics["browser_metrics"]["client_data"].as_str().unwrap())?;
        assert_eq!(client_data["userAgent"]["value"], USER_AGENT);
        assert_eq!(client_data["browserEngineKind"]["value"], "chromium");
        assert_eq!(metrics["detector_results"].as_object().unwrap().len(), 20);
        assert!(followup_body(&json!({"tok":"x","ch":[{"ty":"captcha"}]})).is_err());
        assert!(followup_body(&json!({"ch":[]})).is_err());
        Ok(())
    }

    #[tokio::test]
    #[ignore = "opt-in: verifies production Discover loader and typed app-data decoding"]
    async fn live_production_discover() -> Result<()> {
        super::super::args::init_test_args();
        let html = get("https://bandcamp.com/discover").await?;
        let doc = scraper::Html::parse_document(&html);
        let selector = scraper::Selector::parse("div#DiscoverApp[data-blob]").unwrap();
        let blob = doc
            .select(&selector)
            .next()
            .context("Missing app")?
            .value()
            .attr("data-blob")
            .context("Missing blob")?;
        let data: crate::models::bc_discover_index::DiscoverIndexRequest =
            serde_json::from_str(blob).context("Typed Discover decode failed")?;
        ensure!(
            !data.app_data.initial_state.genres.is_empty(),
            "Missing genres"
        );
        eprintln!(
            "Production Discover loader: typed appData decoded, {} genres",
            data.app_data.initial_state.genres.len()
        );
        Ok(())
    }
}
