//! Thin wrapper over the handful of Slack Web API methods this bridge
//! needs. Not a general-purpose Slack client — just enough to open a
//! Socket Mode connection, resolve the family allowlist, and post replies.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ConnectionsOpen {
    url: String,
}

#[derive(Debug, Deserialize)]
struct AuthTest {
    user_id: String,
}

#[derive(Debug, Deserialize)]
struct LookupByEmail {
    user: SlackUser,
}

#[derive(Debug, Deserialize)]
struct SlackUser {
    id: String,
}

#[derive(Clone)]
pub struct SlackClient {
    http: reqwest::Client,
    bot_token: String,
    app_token: String,
}

impl SlackClient {
    pub fn new(bot_token: String, app_token: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            bot_token,
            app_token,
        }
    }

    async fn call<T>(&self, method: &str, token: &str) -> Result<T, String>
    where
        T: for<'de> Deserialize<'de>,
    {
        let response = self
            .http
            .post(format!("https://slack.com/api/{method}"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|error| format!("{method} request failed: {error}"))?;
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|error| format!("{method} response was not valid JSON: {error}"))?;
        parse_envelope(method, body)
    }

    /// Opens a fresh Socket Mode connection and returns the one-shot
    /// WebSocket URL to connect to (each URL is valid for a single
    /// connection attempt).
    pub async fn open_socket_url(&self) -> Result<String, String> {
        let connection: ConnectionsOpen =
            self.call("apps.connections.open", &self.app_token).await?;
        Ok(connection.url)
    }

    pub async fn bot_user_id(&self) -> Result<String, String> {
        let auth: AuthTest = self.call("auth.test", &self.bot_token).await?;
        Ok(auth.user_id)
    }

    pub async fn lookup_user_id_by_email(&self, email: &str) -> Result<Option<String>, String> {
        let response = self
            .http
            .get("https://slack.com/api/users.lookupByEmail")
            .bearer_auth(&self.bot_token)
            .query(&[("email", email)])
            .send()
            .await
            .map_err(|error| format!("users.lookupByEmail request failed: {error}"))?;
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|error| format!("users.lookupByEmail response was not valid JSON: {error}"))?;
        if body.get("ok").and_then(serde_json::Value::as_bool) == Some(false)
            && body.get("error").and_then(serde_json::Value::as_str) == Some("users_not_found")
        {
            return Ok(None);
        }
        let found: LookupByEmail = parse_envelope("users.lookupByEmail", body)?;
        Ok(Some(found.user.id))
    }

    pub async fn post_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<(), String> {
        let mut body = serde_json::json!({
            "channel": channel,
            "text": text,
        });
        if let Some(thread_ts) = thread_ts {
            body["thread_ts"] = serde_json::Value::String(thread_ts.to_string());
        }
        let response = self
            .http
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await
            .map_err(|error| format!("chat.postMessage request failed: {error}"))?;
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|error| format!("chat.postMessage response was not valid JSON: {error}"))?;
        let _: serde_json::Value = parse_envelope("chat.postMessage", body)?;
        Ok(())
    }
}

/// Slack Web API responses are `{"ok": true, ...fields} | {"ok": false,
/// "error": "..."}`. On success, re-deserializes the same JSON value into
/// `T` (extra keys like `ok` are ignored by default).
fn parse_envelope<T>(method: &str, body: serde_json::Value) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    let ok = body
        .get("ok")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !ok {
        let error = body
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("no error detail");
        return Err(format!("{method} failed: {error}"));
    }
    serde_json::from_value(body)
        .map_err(|error| format!("{method} response shape mismatch: {error}"))
}
