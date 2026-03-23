//! OAuth2 authorization code flow with PKCE for desktop apps.
//!
//! Supports Microsoft (Azure AD) and Google. Flow:
//! 1. Generate PKCE verifier + challenge (CSPRNG + SHA-256)
//! 2. Open browser to authorize URL with state parameter
//! 3. Listen on localhost for redirect callback, validate state
//! 4. Exchange auth code for access_token + refresh_token
//! 5. Return tokens to caller for storage

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::info;

// ── Constants ────────────────────────────────────────────────────────────────

const OAUTH_LISTEN_PORT: u16 = 18523;
const REDIRECT_URI: &str = "http://localhost:18523/oauth/callback";

/// Microsoft OAuth2 endpoints (common tenant for multi-tenant apps)
pub mod microsoft {
    pub const AUTHORIZE_URL: &str =
        "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
    pub const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
    pub const SCOPES: &str =
        "https://outlook.office365.com/IMAP.AccessAsUser.All https://outlook.office365.com/SMTP.Send offline_access openid email";
    /// Thunderbird's public client ID — works for most tenants without admin consent
    pub const DEFAULT_CLIENT_ID: &str = "08162f7c-0fd2-4200-a84a-f25a4db0b584";
}

/// Google OAuth2 endpoints
pub mod google {
    pub const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
    pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
    pub const SCOPES: &str = "https://mail.google.com/ openid email";
    /// User must supply their own client_id/secret via Settings.
    /// These are placeholders that will fail — forces explicit configuration.
    pub const DEFAULT_CLIENT_ID: &str = "";
    pub const DEFAULT_CLIENT_SECRET: &str = "";
}

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OAuthProvider {
    Microsoft,
    Google,
}

impl OAuthProvider {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "microsoft" => Some(Self::Microsoft),
            "google" => Some(Self::Google),
            _ => None,
        }
    }

    pub fn authorize_url(&self) -> &str {
        match self {
            Self::Microsoft => microsoft::AUTHORIZE_URL,
            Self::Google => google::AUTHORIZE_URL,
        }
    }

    pub fn token_url(&self) -> &str {
        match self {
            Self::Microsoft => microsoft::TOKEN_URL,
            Self::Google => google::TOKEN_URL,
        }
    }

    pub fn scopes(&self) -> &str {
        match self {
            Self::Microsoft => microsoft::SCOPES,
            Self::Google => google::SCOPES,
        }
    }

    pub fn default_client_id(&self) -> &str {
        match self {
            Self::Microsoft => microsoft::DEFAULT_CLIENT_ID,
            Self::Google => google::DEFAULT_CLIENT_ID,
        }
    }

    pub fn imap_host(&self) -> &str {
        match self {
            Self::Microsoft => "outlook.office365.com",
            Self::Google => "imap.gmail.com",
        }
    }

    pub fn smtp_host(&self) -> &str {
        match self {
            Self::Microsoft => "smtp.office365.com",
            Self::Google => "smtp.gmail.com",
        }
    }
}

// ── PKCE (RFC 7636) ──────────────────────────────────────────────────────────

/// Generate PKCE verifier (CSPRNG) and S256 challenge (SHA-256).
fn generate_pkce() -> (String, String) {
    let mut verifier_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut verifier_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

    let hash = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hash);

    (verifier, challenge)
}

/// Generate a cryptographically random state token for CSRF protection.
fn generate_state() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

// ── OAuth2 Flow ──────────────────────────────────────────────────────────────

/// Build the authorization URL to open in the user's browser.
pub fn build_authorize_url(
    provider: OAuthProvider,
    client_id: &str,
    code_challenge: &str,
    state: &str,
) -> String {
    let scopes = urlencoding::encode(provider.scopes());
    let redirect = urlencoding::encode(REDIRECT_URI);
    let client_id_enc = urlencoding::encode(client_id);
    let challenge_enc = urlencoding::encode(code_challenge);
    let state_enc = urlencoding::encode(state);

    format!(
        "{}?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256&response_mode=query&prompt=select_account",
        provider.authorize_url(),
        client_id_enc,
        redirect,
        scopes,
        state_enc,
        challenge_enc,
    )
}

/// Start the full OAuth2 flow: open browser, listen for callback, exchange code.
/// Returns tokens on success.
pub async fn start_oauth_flow(
    provider: OAuthProvider,
    client_id: Option<&str>,
) -> Result<OAuthTokens> {
    let client_id = client_id.unwrap_or(provider.default_client_id()).to_string();
    if client_id.is_empty() {
        return Err(anyhow!("No OAuth2 client_id configured. Please set one in Settings."));
    }

    let (verifier, challenge) = generate_pkce();
    let state = generate_state();
    let auth_url = build_authorize_url(provider, &client_id, &challenge, &state);

    info!("Opening OAuth2 authorize URL in browser");
    open::that(&auth_url).context("Failed to open browser")?;

    // Listen for the callback and validate state
    let (code, returned_state) = listen_for_callback().await?;
    if returned_state.as_deref() != Some(state.as_str()) {
        return Err(anyhow!("OAuth2 state mismatch — possible CSRF attack"));
    }
    info!("Received OAuth2 authorization code");

    // Exchange code for tokens
    let tokens = exchange_code(provider, &client_id, &code, &verifier).await?;
    info!("OAuth2 token exchange successful");

    Ok(tokens)
}

/// Listen on localhost for the OAuth2 redirect callback.
/// Returns (code, state) from the query params.
async fn listen_for_callback() -> Result<(String, Option<String>)> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind(format!("127.0.0.1:{OAUTH_LISTEN_PORT}"))
        .await
        .context("Failed to bind OAuth callback listener")?;

    info!("OAuth2 callback listener on port {OAUTH_LISTEN_PORT}");

    // Wait for one connection with timeout
    let (mut stream, _addr) = tokio::time::timeout(
        std::time::Duration::from_secs(300), // 5 min timeout
        listener.accept(),
    )
    .await
    .map_err(|_| anyhow!("OAuth2 callback timeout (5 min)"))?
    .context("Accept failed")?;

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.context("Read failed")?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Verify this is a GET to /oauth/callback
    let first_line = request.lines().next().unwrap_or("");
    if !first_line.contains("/oauth/callback") {
        return Err(anyhow!("Unexpected callback path: {first_line}"));
    }

    // Parse code and state
    let code = extract_query_param(&request, "code").ok_or_else(|| {
        let error = extract_query_param(&request, "error").unwrap_or_default();
        let desc = extract_query_param(&request, "error_description").unwrap_or_default();
        anyhow!("OAuth2 error: {error} — {desc}")
    })?;
    let returned_state = extract_query_param(&request, "state");

    // Send success response to browser
    let html = r#"<!DOCTYPE html><html><body style="font-family:system-ui;text-align:center;padding:60px">
<h2>Authorization Successful</h2><p>You can close this tab and return to Sage.</p>
<script>setTimeout(()=>window.close(),2000)</script></body></html>"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );
    let _ = stream.write_all(response.as_bytes()).await;

    Ok((code, returned_state))
}

fn extract_query_param(request: &str, key: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        if kv.next()? == key {
            let val = kv.next().unwrap_or("");
            return Some(urlencoding::decode(val).unwrap_or_default().to_string());
        }
    }
    None
}

/// Exchange authorization code for tokens.
async fn exchange_code(
    provider: OAuthProvider,
    client_id: &str,
    code: &str,
    verifier: &str,
) -> Result<OAuthTokens> {
    let mut params = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", REDIRECT_URI),
        ("client_id", client_id),
        ("code_verifier", verifier),
    ];

    // Google requires client_secret even for "desktop" apps
    let secret;
    if provider == OAuthProvider::Google {
        secret = google::DEFAULT_CLIENT_SECRET.to_string();
        if !secret.is_empty() {
            params.push(("client_secret", &secret));
        }
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(provider.token_url())
        .form(&params)
        .send()
        .await
        .context("Token exchange request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::debug!("Token exchange error body: {body}");
        return Err(anyhow!("Token exchange failed (HTTP {status})"));
    }

    let body: serde_json::Value = resp.json().await.context("Parse token response")?;
    parse_token_response(&body)
}

/// Refresh an expired access token using the refresh token.
pub async fn refresh_access_token(
    provider: OAuthProvider,
    client_id: &str,
    refresh_token: &str,
) -> Result<OAuthTokens> {
    let mut params = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
    ];

    let secret;
    if provider == OAuthProvider::Google {
        secret = google::DEFAULT_CLIENT_SECRET.to_string();
        if !secret.is_empty() {
            params.push(("client_secret", &secret));
        }
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(provider.token_url())
        .form(&params)
        .send()
        .await
        .context("Token refresh request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::debug!("Token refresh error body: {body}");
        return Err(anyhow!("Token refresh failed (HTTP {status})"));
    }

    let body: serde_json::Value = resp.json().await.context("Parse refresh response")?;
    let mut tokens = parse_token_response(&body)?;
    // Refresh response may not include a new refresh_token — keep the old one
    if tokens.refresh_token.is_none() {
        tokens.refresh_token = Some(refresh_token.to_string());
    }
    Ok(tokens)
}

fn parse_token_response(body: &serde_json::Value) -> Result<OAuthTokens> {
    let access_token = body["access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("No access_token in response"))?
        .to_string();

    let refresh_token = body["refresh_token"].as_str().map(String::from);

    let expires_at = body["expires_in"].as_u64().map(|secs| {
        let expiry = chrono::Utc::now() + chrono::Duration::seconds(secs as i64);
        expiry.to_rfc3339()
    });

    // Extract email from id_token JWT payload.
    // SAFETY: id_token is received directly over HTTPS from the token endpoint,
    // so transport-level trust is sufficient. We do NOT use this for access control,
    // only for display (pre-filling the email field in settings).
    let email = body["id_token"]
        .as_str()
        .and_then(extract_email_from_jwt);

    Ok(OAuthTokens {
        access_token,
        refresh_token,
        expires_at,
        email,
    })
}

/// Extract email from JWT id_token payload. Decoded without signature verification
/// because the token comes directly from the HTTPS token endpoint (transport trust).
/// Used only for display purposes (email field), NOT for access control.
fn extract_email_from_jwt(jwt: &str) -> Option<String> {
    let payload = jwt.split('.').nth(1)?;
    // JWT base64 may need padding
    let padded = match payload.len() % 4 {
        2 => format!("{payload}=="),
        3 => format!("{payload}="),
        _ => payload.to_string(),
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(&padded)
        .or_else(|_| URL_SAFE_NO_PAD.decode(payload))
        .ok()?;
    let val: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    val["email"]
        .as_str()
        .or_else(|| val["preferred_username"].as_str())
        .or_else(|| val["upn"].as_str())
        .map(String::from)
}

/// Build XOAUTH2 SASL token for IMAP AUTHENTICATE.
/// Format: "user={email}\x01auth=Bearer {token}\x01\x01"
pub fn build_xoauth2_token(email: &str, access_token: &str) -> String {
    format!("user={email}\x01auth=Bearer {access_token}\x01\x01")
}

/// Check if an access token is expired (with 5-minute buffer).
pub fn is_token_expired(expires_at: Option<&str>) -> bool {
    match expires_at {
        None => true,
        Some(s) => {
            let expiry = chrono::DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| {
                    tracing::warn!("Failed to parse token expiry: {s}");
                    chrono::Utc::now()
                });
            expiry < chrono::Utc::now() + chrono::Duration::minutes(5)
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation_uses_csprng() {
        let (v1, c1) = generate_pkce();
        let (v2, c2) = generate_pkce();
        assert!(!v1.is_empty());
        assert!(!c1.is_empty());
        assert_ne!(v1, v2, "PKCE verifiers must be unique");
        assert_ne!(c1, c2, "PKCE challenges must be unique");
    }

    #[test]
    fn test_sha256_known_value() {
        let hash = Sha256::digest(b"");
        let result = URL_SAFE_NO_PAD.encode(hash);
        assert_eq!(result, "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU");
    }

    #[test]
    fn test_build_authorize_url_includes_state() {
        let url = build_authorize_url(
            OAuthProvider::Microsoft,
            "test-client-id",
            "test-challenge",
            "test-state-123",
        );
        assert!(url.contains("login.microsoftonline.com"));
        assert!(url.contains("test-client-id"));
        assert!(url.contains("test-challenge"));
        assert!(url.contains("test-state-123"));
        assert!(url.contains("code_challenge_method=S256"));
    }

    #[test]
    fn test_build_authorize_url_google() {
        let url = build_authorize_url(
            OAuthProvider::Google,
            "google-client-id",
            "challenge",
            "state",
        );
        assert!(url.contains("accounts.google.com"));
        assert!(url.contains("google-client-id"));
    }

    #[test]
    fn test_extract_query_param() {
        let req = "GET /oauth/callback?code=abc123&state=xyz HTTP/1.1\r\nHost: localhost\r\n";
        assert_eq!(extract_query_param(req, "code"), Some("abc123".into()));
        assert_eq!(extract_query_param(req, "state"), Some("xyz".into()));
        assert_eq!(extract_query_param(req, "missing"), None);
    }

    #[test]
    fn test_build_xoauth2_token() {
        let token = build_xoauth2_token("user@example.com", "ya29.access");
        assert_eq!(
            token,
            "user=user@example.com\x01auth=Bearer ya29.access\x01\x01"
        );
    }

    #[test]
    fn test_is_token_expired_none() {
        assert!(is_token_expired(None));
    }

    #[test]
    fn test_is_token_expired_future() {
        let future = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        assert!(!is_token_expired(Some(&future)));
    }

    #[test]
    fn test_is_token_expired_past() {
        let past = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        assert!(is_token_expired(Some(&past)));
    }

    #[test]
    fn test_extract_email_from_jwt() {
        let payload = serde_json::json!({"email": "user@test.com", "sub": "123"});
        let encoded = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let jwt = format!("header.{encoded}.signature");
        assert_eq!(extract_email_from_jwt(&jwt), Some("user@test.com".into()));
    }

    #[test]
    fn test_provider_from_str() {
        assert_eq!(
            OAuthProvider::from_str("microsoft"),
            Some(OAuthProvider::Microsoft)
        );
        assert_eq!(
            OAuthProvider::from_str("google"),
            Some(OAuthProvider::Google)
        );
        assert_eq!(OAuthProvider::from_str("unknown"), None);
    }

    #[test]
    fn test_generate_state_unique() {
        let s1 = generate_state();
        let s2 = generate_state();
        assert_ne!(s1, s2);
        assert!(s1.len() >= 16);
    }
}
