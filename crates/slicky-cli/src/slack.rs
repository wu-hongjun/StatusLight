//! Slack OAuth login/logout/status for the CLI.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use anyhow::{bail, Context, Result};
use slicky_core::Config;

const SLACK_CLIENT_ID: &str = env!("SLACK_CLIENT_ID");
const SLACK_CLIENT_SECRET: &str = env!("SLACK_CLIENT_SECRET");

const REDIRECT_PORT: u16 = 19876;
const REDIRECT_URI: &str = "http://127.0.0.1:19876/callback";

/// `slicky slack login` — open browser for OAuth, exchange code for token.
pub fn login() -> Result<()> {
    let mut config = Config::load()?;

    if config.slack.token.is_some() {
        println!("Already logged in. Run `slicky slack logout` first to re-authenticate.");
        return Ok(());
    }

    // Bind listener before opening browser so we fail fast if port is taken.
    let listener = TcpListener::bind(("127.0.0.1", REDIRECT_PORT))
        .context("failed to bind callback listener (is port 19876 in use?)")?;

    let auth_url = format!(
        "https://slack.com/oauth/v2/authorize?client_id={}&user_scope=users.profile:read,users.profile:write&redirect_uri={}",
        SLACK_CLIENT_ID, REDIRECT_URI
    );

    println!("Opening browser for Slack authorization...");
    open::that(&auth_url).context("failed to open browser")?;
    println!("Waiting for Slack callback on port {REDIRECT_PORT}...");

    // Accept exactly one connection.
    let (mut stream, _) = listener.accept().context("failed to accept callback")?;

    // Read the HTTP request line to extract the code.
    let reader = BufReader::new(&stream);
    let request_line = reader
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty request"))?
        .context("failed to read request line")?;

    let code = extract_code(&request_line)?;

    // Send success response to browser before exchanging the code.
    let html = "<html><body><h2>Success!</h2><p>You can close this tab and return to your terminal.</p></body></html>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );
    let _ = stream.write_all(response.as_bytes());

    // Exchange authorization code for token.
    println!("Exchanging code for token...");
    let token = exchange_code(&code)?;

    config.slack.token = Some(token);
    config.save()?;

    println!("Slack login successful! Token saved to config.");
    Ok(())
}

/// `slicky slack logout` — remove token from config.
pub fn logout() -> Result<()> {
    let mut config = Config::load()?;
    if config.slack.token.is_none() {
        println!("Not logged in.");
        return Ok(());
    }
    config.slack.token = None;
    config.save()?;
    println!("Slack token removed.");
    Ok(())
}

/// `slicky slack status` — show connection state.
pub fn status() -> Result<()> {
    let config = Config::load()?;
    match &config.slack.token {
        Some(token) => {
            let masked = if token.len() > 12 {
                format!("{}...{}", &token[..8], &token[token.len() - 4..])
            } else {
                "****".to_string()
            };
            println!("Slack: logged in (token: {masked})");
            println!("Poll interval: {}s", config.slack.poll_interval_secs);
        }
        None => {
            println!("Slack: not logged in");
            println!("Run `slicky slack login` to connect.");
        }
    }
    Ok(())
}

/// `slicky slack set-status` — set Slack status text and emoji.
pub fn set_status(text: &str, emoji: &str) -> Result<()> {
    let config = Config::load()?;
    let token = config.slack.token.ok_or_else(|| {
        anyhow::anyhow!("not logged in to Slack — run `slicky slack login` first")
    })?;

    let body = serde_json::json!({
        "profile": {
            "status_text": text,
            "status_emoji": emoji,
            "status_expiration": 0
        }
    });

    let json_body = serde_json::to_string(&body).context("failed to serialize request")?;

    let resp = ureq::post("https://slack.com/api/users.profile.set")
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json; charset=utf-8")
        .send(json_body.as_bytes())
        .context("failed to set Slack status")?;

    let json: serde_json::Value = serde_json::from_reader(resp.into_body().into_reader())
        .context("failed to parse Slack response")?;

    if !json["ok"].as_bool().unwrap_or(false) {
        let err = json["error"].as_str().unwrap_or("unknown error");
        bail!("Slack API error: {err}");
    }

    if text.is_empty() {
        println!("Slack status cleared");
    } else {
        println!("Slack status set: {emoji} {text}");
    }
    Ok(())
}

/// `slicky slack clear-status` — clear Slack status.
pub fn clear_status() -> Result<()> {
    set_status("", "")
}

/// Extract the `code` query parameter from an HTTP GET request line.
fn extract_code(request_line: &str) -> Result<String> {
    // e.g. "GET /callback?code=XXXX HTTP/1.1"
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("malformed HTTP request"))?;

    let query = path
        .split('?')
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("no query string in callback"))?;

    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("code=") {
            return Ok(value.to_string());
        }
    }

    // Check for error parameter from Slack.
    for pair in query.split('&') {
        if let Some(value) = pair.strip_prefix("error=") {
            bail!("Slack authorization denied: {value}");
        }
    }

    bail!("no code parameter in callback URL")
}

/// Exchange an OAuth authorization code for a user access token.
fn exchange_code(code: &str) -> Result<String> {
    let body = format!(
        "client_id={}&client_secret={}&code={}&redirect_uri={}",
        SLACK_CLIENT_ID, SLACK_CLIENT_SECRET, code, REDIRECT_URI
    );

    let resp = ureq::post("https://slack.com/api/oauth.v2.access")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(body.as_bytes())
        .context("failed to exchange code for token")?;

    let json: serde_json::Value = serde_json::from_reader(resp.into_body().into_reader())
        .context("failed to parse token response")?;

    if !json["ok"].as_bool().unwrap_or(false) {
        let err = json["error"].as_str().unwrap_or("unknown error");
        bail!("Slack token exchange failed: {err}");
    }

    json["authed_user"]["access_token"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("missing access_token in response"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_code_standard_callback() {
        let line = "GET /callback?code=abc123def456 HTTP/1.1";
        let code = extract_code(line).unwrap();
        assert_eq!(code, "abc123def456");
    }

    #[test]
    fn extract_code_with_extra_params() {
        let line = "GET /callback?state=xyz&code=mycode789&other=val HTTP/1.1";
        let code = extract_code(line).unwrap();
        assert_eq!(code, "mycode789");
    }

    #[test]
    fn extract_code_error_from_slack() {
        let line = "GET /callback?error=access_denied HTTP/1.1";
        let err = extract_code(line).unwrap_err();
        assert!(
            err.to_string().contains("access_denied"),
            "should contain the error reason"
        );
    }

    #[test]
    fn extract_code_no_query_string() {
        let line = "GET /callback HTTP/1.1";
        assert!(extract_code(line).is_err());
    }

    #[test]
    fn extract_code_no_code_param() {
        let line = "GET /callback?state=xyz HTTP/1.1";
        let err = extract_code(line).unwrap_err();
        assert!(err.to_string().contains("no code parameter"));
    }

    #[test]
    fn extract_code_malformed_request() {
        let line = "INVALID";
        // Only one word, nth(1) returns None.
        assert!(extract_code(line).is_err());
    }

    #[test]
    fn extract_code_empty_code() {
        let line = "GET /callback?code= HTTP/1.1";
        let code = extract_code(line).unwrap();
        assert_eq!(code, "");
    }
}
