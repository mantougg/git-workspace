use std::path::PathBuf;

/// Manages SSH credentials for Git remote operations (fetch/pull/push).
///
/// Attempts credentials in the following order:
/// 1. SSH agent (if available)
/// 2. SSH key files from ~/.ssh/ (id_ed25519, id_rsa, id_ecdsa, id_dsa)
/// 3. Default credentials (for HTTPS)
pub struct SshCredentials {
    key_paths: Vec<PathBuf>,
}

impl SshCredentials {
    /// Create a new SshCredentials, auto-detecting common SSH key paths.
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_default();

        let key_paths = vec![
            home.join(".ssh/id_ed25519"),
            home.join(".ssh/id_rsa"),
            home.join(".ssh/id_ecdsa"),
            home.join(".ssh/id_dsa"),
        ];

        log::debug!(
            "SshCredentials initialized with {} key paths from {:?}",
            key_paths.len(),
            home
        );

        SshCredentials { key_paths }
    }

    /// Set up the credentials callback on a `RemoteCallbacks` object.
    ///
    /// The callback will be invoked by libgit2 when authentication is needed.
    /// It tries SSH agent, then SSH key files, then default credentials.
    ///
    /// Note: network operations (fetch/pull/push) currently run through the
    /// `git` CLI (`run_git`), which handles authentication via the user's
    /// git installation (credential manager / ssh-agent). This method is kept
    /// for potential libgit2-based paths and future per-repo SSH customization.
    #[allow(dead_code)]
    pub fn setup_callbacks<'a>(&self, callbacks: &mut git2::RemoteCallbacks<'a>) {
        let key_paths = self.key_paths.clone();

        callbacks.credentials(move |url, username, _allowed_types| {
            log::debug!(
                "Credentials requested for {} (user: {:?})",
                url,
                username
            );

            // 1. Try SSH Agent
            if let Some(user) = username {
                if let Ok(cred) = git2::Cred::ssh_key_from_agent(user) {
                    log::debug!("Authenticated via SSH agent");
                    return Ok(cred);
                }
            }

            // 2. Try SSH key files
            let user = username.unwrap_or("git");
            for key_path in &key_paths {
                if key_path.exists() {
                    let pub_key = key_path.with_extension("pub");
                    match git2::Cred::ssh_key(
                        user,
                        Some(&pub_key),
                        key_path,
                        None, // No passphrase support yet
                    ) {
                        Ok(cred) => {
                            log::debug!("Authenticated via SSH key: {:?}", key_path);
                            return Ok(cred);
                        }
                        Err(e) => {
                            log::warn!(
                                "Failed to use SSH key {:?}: {}",
                                key_path,
                                e
                            );
                        }
                    }
                }
            }

            // 3. Try default credentials (for HTTPS with credential helper)
            if let Ok(cred) = git2::Cred::default() {
                log::debug!("Using default credentials");
                return Ok(cred);
            }

            Err(git2::Error::from_str(
                "No suitable credentials found for SSH/HTTPS authentication",
            ))
        });
    }
}

impl Default for SshCredentials {
    fn default() -> Self {
        Self::new()
    }
}
