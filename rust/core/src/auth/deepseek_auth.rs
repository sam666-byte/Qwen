//! DeepSeek Session Auth
//!
//! DeepSeek provides FREE access via chat.deepseek.com session cookies.
//! Login at chat.deepseek.com → capture ds_session_id cookie → use as auth
//! for api.deepseek.com/chat/completions.
//!
//! This is the same "free via browser login" pattern as Qwen OAuth.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

/// DeepSeek session cookie obtained after logging in at chat.deepseek.com
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepSeekSession {
    /// The full cookie header string (e.g. "ds_session_id=abc123; ...")
    pub session_cookie: String,
    /// Model to use with DeepSeek
    pub model: String,
    /// When this session was saved (Unix timestamp)
    pub saved_at: u64,
}

impl DeepSeekSession {
    /// Check if session is still fresh (cookies don't have a fixed expiry like OAuth tokens,
    /// but we treat sessions older than 24h as potentially stale)
    pub fn is_stale(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Sessions older than 24 hours are considered stale
        now > self.saved_at + 86400
    }

    /// Extract just the ds_session_id value for display
    pub fn session_id_preview(&self) -> String {
        for part in self.session_cookie.split(';') {
            let part = part.trim();
            if let Some(value) = part.strip_prefix("ds_session_id=") {
                if value.len() > 12 {
                    return format!("{}...{}", &value[..6], &value[value.len()-4..]);
                }
                return value.to_string();
            }
        }
        "✓ (session active)".to_string()
    }
}

pub struct DeepSeekAuth {
    pub session_path: PathBuf,
    pub session: Option<DeepSeekSession>,
}

impl DeepSeekAuth {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            session_path: config_dir.join("deepseek-session.json"),
            session: None,
        }
    }

    /// Load session from file
    pub fn load_session(&mut self) -> Result<Option<DeepSeekSession>> {
        if !self.session_path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&self.session_path)?;
        let session: DeepSeekSession = serde_json::from_str(&content)?;

        if session.session_cookie.is_empty() {
            info!("Session cookie is empty");
            return Ok(None);
        }

        self.session = Some(session.clone());
        Ok(Some(session))
    }

    /// Save session cookie to file
    pub fn save_session(&self, session_cookie: &str, model: Option<&str>) -> Result<()> {
        if let Some(parent) = self.session_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let session = DeepSeekSession {
            session_cookie: session_cookie.to_string(),
            model: model.unwrap_or("deepseek-chat").to_string(),
            saved_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        let json = serde_json::to_string_pretty(&session)?;
        std::fs::write(&self.session_path, json)?;
        info!("DeepSeek session saved");
        Ok(())
    }

    /// Check if authenticated (has valid session cookie)
    pub fn is_authenticated(&mut self) -> bool {
        self.load_session()
            .map(|s| s.is_some())
            .unwrap_or(false)
    }

    /// Get the session cookie string (for use as Cookie header)
    pub fn get_session_cookie(&mut self) -> Result<String> {
        if let Some(session) = &self.session {
            if !session.session_cookie.is_empty() {
                return Ok(session.session_cookie.clone());
            }
        }
        if let Some(session) = self.load_session()? {
            return Ok(session.session_cookie);
        }
        anyhow::bail!(
            "DeepSeek not authenticated. Run: archclaw auth deepseek\n\
             This will open chat.deepseek.com in your browser for free login."
        )
    }

    /// Get the configured model
    pub fn get_model(&mut self) -> Result<String> {
        if self.session.is_none() && self.session_path.exists() {
            let content = std::fs::read_to_string(&self.session_path)?;
            self.session = Some(serde_json::from_str(&content)?);
        }
        Ok(self.session.as_ref()
            .map(|s| s.model.clone())
            .unwrap_or_else(|| "deepseek-chat".to_string()))
    }

    /// Remove session (logout)
    pub fn logout(&mut self) -> Result<()> {
        if self.session_path.exists() {
            std::fs::remove_file(&self.session_path)?;
        }
        self.session = None;
        info!("DeepSeek session removed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_save_and_load_session() {
        let dir = std::env::temp_dir().join("deepseek_session_test");
        let _ = fs::create_dir_all(&dir);

        let auth = DeepSeekAuth::new(dir.clone());
        auth.save_session(
            "ds_session_id=abc123def456; other=cookie",
            Some("deepseek-chat")
        ).unwrap();

        let dir2 = dir.clone();
        let mut auth2 = DeepSeekAuth::new(dir);
        let session = auth2.load_session().unwrap();
        assert!(session.is_some());
        assert_eq!(session.unwrap().session_cookie, "ds_session_id=abc123def456; other=cookie");

        let _ = fs::remove_dir_all(dir2);
    }

    #[test]
    fn test_logout() {
        let dir = std::env::temp_dir().join("deepseek_logout_test");
        let _ = fs::create_dir_all(&dir);

        let mut auth = DeepSeekAuth::new(dir.clone());
        auth.save_session("ds_session_id=test", None).unwrap();
        assert!(auth.session_path.exists());

        auth.logout().unwrap();
        assert!(!auth.session_path.exists());
        assert!(auth.session.is_none());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_is_authenticated() {
        let dir = std::env::temp_dir().join("deepseek_auth_test");
        let _ = fs::create_dir_all(&dir);

        let mut auth = DeepSeekAuth::new(dir.clone());
        assert!(!auth.is_authenticated());

        auth.save_session("ds_session_id=abc123", None).unwrap();
        let dir2 = dir.clone();
        let mut auth2 = DeepSeekAuth::new(dir);
        assert!(auth2.is_authenticated());

        let _ = fs::remove_dir_all(dir2);
    }

    #[test]
    fn test_session_id_preview() {
        let session = DeepSeekSession {
            session_cookie: "ds_session_id=abcdef1234567890xyz; other=val".to_string(),
            model: "deepseek-chat".to_string(),
            saved_at: 0,
        };
        let preview = session.session_id_preview();
        assert!(preview.contains("abcdef"));
        assert!(preview.contains("xyz"));
    }
}
