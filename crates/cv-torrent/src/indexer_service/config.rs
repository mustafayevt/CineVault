//! The indexer service's `ServerConfig.json`: the few keys CineVault owns are written, every other
//! key (the service keeps its own settings in the same file) is left as it was.

use std::fs;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::ServiceError;

/// The file name of the service's configuration.
pub const CONFIG_FILE: &str = "ServerConfig.json";

/// The address the service listens on: this computer only.
pub const BIND_ADDRESS: &str = "127.0.0.1";

/// The length of the API key CineVault gives the service.
pub const API_KEY_LEN: usize = 32;

/// Reads and writes `ServerConfig.json`.
#[derive(Debug, Clone, Copy)]
pub struct ServerConfigFile;

impl ServerConfigFile {
    /// The folder that holds `ServerConfig.json` for a data folder of ours. On Windows the service
    /// is given `-d <data>` and keeps its files there. On macOS it has no such option, so it is
    /// started with `XDG_CONFIG_HOME=<data>` and keeps them in `<data>/Jackett`.
    #[must_use]
    pub fn config_dir(data_dir: &Path) -> PathBuf {
        if cfg!(windows) {
            data_dir.to_path_buf()
        } else {
            data_dir.join("Jackett")
        }
    }

    /// The API key already in the file, when it is one of ours (32 letters and digits).
    #[must_use]
    pub fn read_api_key(config_dir: &Path) -> Option<String> {
        let text = fs::read_to_string(config_dir.join(CONFIG_FILE)).ok()?;
        let value: Value = serde_json::from_str(&text).ok()?;
        let key = value.get("APIKey")?.as_str()?;
        is_api_key(key).then(|| key.to_owned())
    }

    /// Writes the keys CineVault owns into `<config_dir>/ServerConfig.json`, creating the file
    /// (and the folder) when needed and keeping every other key as it was:
    ///
    /// * `Port`, and `LocalBindAddress` `127.0.0.1` with `AllowExternal` false, so only this
    ///   computer can reach it;
    /// * `APIKey`;
    /// * `UpdateDisabled` true (CineVault updates the service itself) and `CacheEnabled` true;
    /// * `FlareSolverrUrl`: the challenge solver address, or null for none.
    ///
    /// A file that is not valid JSON is moved aside (`ServerConfig.json.broken`) and replaced.
    /// The new file replaces the old one in one rename.
    ///
    /// # Errors
    /// [`ServiceError::Io`] when the folder or file cannot be written.
    pub fn write_or_patch(
        config_dir: &Path,
        port: u16,
        api_key: &str,
        solver_url: Option<&str>,
    ) -> Result<(), ServiceError> {
        fs::create_dir_all(config_dir).map_err(|error| {
            ServiceError::io(format!("Could not create {}", config_dir.display()), error)
        })?;
        let path = config_dir.join(CONFIG_FILE);
        let mut map = match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(Value::Object(map)) => map,
                _ => {
                    let _ = fs::rename(&path, config_dir.join(format!("{CONFIG_FILE}.broken")));
                    Map::new()
                }
            },
            Err(_) => Map::new(),
        };
        map.insert("Port".to_owned(), Value::from(port));
        map.insert("LocalBindAddress".to_owned(), Value::from(BIND_ADDRESS));
        map.insert("AllowExternal".to_owned(), Value::from(false));
        map.insert("APIKey".to_owned(), Value::from(api_key));
        map.insert("UpdateDisabled".to_owned(), Value::from(true));
        map.insert("CacheEnabled".to_owned(), Value::from(true));
        map.insert(
            "FlareSolverrUrl".to_owned(),
            match solver_url.map(str::trim) {
                Some(url) if !url.is_empty() => Value::from(url),
                _ => Value::Null,
            },
        );
        let text = serde_json::to_string_pretty(&Value::Object(map))
            .map_err(|error| ServiceError::Config(error.to_string()))?;
        write_replacing(&path, text.as_bytes())
    }
}

/// Writes `bytes` to a sibling file and renames it over `path`.
fn write_replacing(path: &Path, bytes: &[u8]) -> Result<(), ServiceError> {
    let temp = path.with_extension("json.cv-tmp");
    let mut file = fs::File::create(&temp)
        .map_err(|error| ServiceError::io(format!("Could not write {}", temp.display()), error))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| ServiceError::io(format!("Could not write {}", temp.display()), error))?;
    drop(file);
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        ServiceError::io(format!("Could not replace {}", path.display()), error)
    })
}

/// Whether `text` looks like a key CineVault generated.
fn is_api_key(text: &str) -> bool {
    text.len() == API_KEY_LEN
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// A new random API key: 32 lower-case letters and digits.
///
/// # Errors
/// [`ServiceError::Config`] when the system cannot supply random bytes.
pub fn generate_api_key() -> Result<String, ServiceError> {
    const ALPHABET: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut key = String::with_capacity(API_KEY_LEN);
    let mut bytes = [0_u8; 64];
    while key.len() < API_KEY_LEN {
        getrandom::fill(&mut bytes)
            .map_err(|error| ServiceError::Config(format!("no random bytes: {error}")))?;
        for byte in bytes {
            // 252 is the largest multiple of 36 below 256: bytes above it would favour some letters.
            if byte < 252 && key.len() < API_KEY_LEN {
                key.push(char::from(ALPHABET[usize::from(byte) % ALPHABET.len()]));
            }
        }
    }
    Ok(key)
}

/// A port on the loopback interface that nothing is listening on right now.
///
/// # Errors
/// [`ServiceError::Io`] when no port can be bound.
pub fn free_loopback_port() -> Result<u16, ServiceError> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .map_err(|error| ServiceError::io("Could not find a free port", error))?;
    let port = listener
        .local_addr()
        .map_err(|error| ServiceError::io("Could not find a free port", error))?
        .port();
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(dir: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(dir.join(CONFIG_FILE)).expect("file"))
            .expect("json")
    }

    #[test]
    fn a_new_file_holds_exactly_the_keys_we_own() {
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = temp.path().join("nested").join("cfg");
        ServerConfigFile::write_or_patch(&dir, 9117, "k".repeat(32).as_str(), None).expect("write");
        let value = read(&dir);
        assert_eq!(value["Port"], 9117);
        assert_eq!(value["LocalBindAddress"], "127.0.0.1");
        assert_eq!(value["AllowExternal"], false);
        assert_eq!(value["UpdateDisabled"], true);
        assert_eq!(value["CacheEnabled"], true);
        assert_eq!(value["APIKey"], "k".repeat(32));
        assert!(value["FlareSolverrUrl"].is_null());
        assert_eq!(value.as_object().expect("object").len(), 7);
    }

    #[test]
    fn unknown_keys_survive_a_patch_and_ours_are_replaced() {
        let temp = tempfile::tempdir().expect("temp dir");
        fs::write(
            temp.path().join(CONFIG_FILE),
            r#"{"Port":1,"AllowExternal":true,"LocalBindAddress":"0.0.0.0","AdminPassword":"hash","InstanceId":"abc","Nested":{"a":[1,2]},"BlackholeDir":null}"#,
        )
        .expect("seed");
        ServerConfigFile::write_or_patch(
            temp.path(),
            4000,
            "a".repeat(32).as_str(),
            Some(" http://localhost:8191 "),
        )
        .expect("patch");
        let value = read(temp.path());
        assert_eq!(value["AdminPassword"], "hash");
        assert_eq!(value["InstanceId"], "abc");
        assert_eq!(value["Nested"]["a"][1], 2);
        assert!(value["BlackholeDir"].is_null());
        assert_eq!(value["Port"], 4000);
        assert_eq!(value["AllowExternal"], false);
        assert_eq!(value["LocalBindAddress"], "127.0.0.1");
        assert_eq!(value["FlareSolverrUrl"], "http://localhost:8191");
    }

    #[test]
    fn a_cleared_solver_address_becomes_null() {
        let temp = tempfile::tempdir().expect("temp dir");
        let key = "b".repeat(32);
        ServerConfigFile::write_or_patch(temp.path(), 1, &key, Some("http://x:1")).expect("one");
        ServerConfigFile::write_or_patch(temp.path(), 1, &key, Some("  ")).expect("two");
        assert!(read(temp.path())["FlareSolverrUrl"].is_null());
    }

    #[test]
    fn a_broken_file_is_moved_aside_and_replaced() {
        let temp = tempfile::tempdir().expect("temp dir");
        fs::write(temp.path().join(CONFIG_FILE), "{ not json").expect("seed");
        ServerConfigFile::write_or_patch(temp.path(), 1, &"c".repeat(32), None).expect("write");
        assert_eq!(read(temp.path())["Port"], 1);
        assert_eq!(
            fs::read_to_string(temp.path().join("ServerConfig.json.broken")).expect("kept"),
            "{ not json"
        );
        assert!(!temp.path().join("ServerConfig.json.cv-tmp").exists());
    }

    #[test]
    fn our_key_is_read_back_and_a_stranger_key_is_not() {
        let temp = tempfile::tempdir().expect("temp dir");
        assert_eq!(ServerConfigFile::read_api_key(temp.path()), None);
        let key = generate_api_key().expect("key");
        ServerConfigFile::write_or_patch(temp.path(), 1, &key, None).expect("write");
        assert_eq!(ServerConfigFile::read_api_key(temp.path()), Some(key));
        fs::write(temp.path().join(CONFIG_FILE), r#"{"APIKey":"short"}"#).expect("seed");
        assert_eq!(ServerConfigFile::read_api_key(temp.path()), None);
    }

    #[test]
    fn keys_are_32_characters_and_differ() {
        let one = generate_api_key().expect("one");
        let two = generate_api_key().expect("two");
        assert!(is_api_key(&one));
        assert_ne!(one, two);
    }

    #[test]
    fn the_free_port_can_be_bound() {
        let port = free_loopback_port().expect("port");
        assert!(port > 0);
        TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("still free");
    }

    #[test]
    fn the_config_folder_follows_the_platform() {
        let data = Path::new("data");
        let dir = ServerConfigFile::config_dir(data);
        if cfg!(windows) {
            assert_eq!(dir, data);
        } else {
            assert_eq!(dir, data.join("Jackett"));
        }
    }
}
