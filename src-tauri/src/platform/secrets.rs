//! Secure credential storage: the macOS Keychain, Windows Credential Manager, or the Linux Secret
//! Service (GNOME Keyring, KWallet). Secrets never go into the database or a file.

use crate::error::{AppError, Result};

const SERVICE: &str = "dev.nexus.workspace";

/// The name of this system's store, for messages and the About panel.
pub const BACKEND: &str = if cfg!(target_os = "macos") {
    "macOS Keychain"
} else if cfg!(windows) {
    "Windows Credential Manager"
} else {
    "Secret Service (GNOME Keyring or KWallet)"
};

/// Turn a storage failure into something the user can act on.
fn explain(action: &str, error: keyring::Error) -> AppError {
    let hint = if cfg!(target_os = "linux") {
        " On Linux this needs a running Secret Service: install and unlock GNOME Keyring or KWallet, then try again."
    } else {
        ""
    };
    AppError::Msg(format!("Could not {action} {BACKEND}: {error}.{hint}"))
}

fn entry(id: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, id).map_err(|e| explain("open", e))
}

pub fn get(id: &str) -> Result<Option<String>> {
    match entry(id)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(explain("read the API key from", e)),
    }
}

pub fn set(id: &str, secret: &str) -> Result<()> {
    entry(id)?.set_password(secret).map_err(|e| explain("store the API key in", e))
}

/// Remove a secret. Removing one that is not there is not an error.
pub fn delete(id: &str) -> Result<()> {
    match entry(id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(explain("remove the API key from", e)),
    }
}
