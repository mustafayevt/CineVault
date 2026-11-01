//! Share addresses and the checks on what the user types.
//!
//! The Network share page has one Address field. It takes the forms people copy from File
//! Explorer, Finder or a file manager: `\\nas\movies\Films`, `//nas/movies/Films` and
//! `smb://nas/movies/Films`, a folder inside the share being optional. Every part is checked
//! before any tool starts, which also keeps the tools' arguments unambiguous:
//!
//! * server: a host name (letters, digits, `-` and `_` in dot-separated labels), an IPv4 address,
//!   or an IPv6 address in brackets;
//! * share: 1 to 80 characters without `\ / : * ? " < > |` or control characters, not `.` or `..`;
//! * folder: names without control characters, never `.`, `..` or empty;
//! * user: printable, with an optional `DOMAIN\` part;
//! * password: anything but a line break or a NUL, as it is sent as one line.

use std::fmt;
use std::net::Ipv6Addr;
use std::path::{Path, PathBuf};

use crate::error::{ShareError, ShareErrorKind};

/// Why something typed on the Network share page is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    #[error("Enter the share's address, such as \\\\nas\\movies.")]
    EmptyAddress,
    #[error("Enter the address as \\\\server\\share or smb://server/share.")]
    UnknownForm,
    #[error("Add the share's name after the server, such as \\\\nas\\movies.")]
    MissingShare,
    #[error("The server name is not valid.")]
    BadServer,
    #[error("A server port cannot be given; shares use the standard port.")]
    PortGiven,
    #[error("The share name is not valid.")]
    BadShare,
    #[error("The folder inside the share is not valid.")]
    BadFolder,
    #[error("Enter the password in the Password field, not in the address.")]
    PasswordInAddress,
    #[error("Enter a user name.")]
    EmptyUser,
    #[error("The user name is not valid.")]
    BadUser,
    #[error("The password cannot hold a line break.")]
    BadPassword,
}

/// A share on a server: `\\nas\movies`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShareAddress {
    server: String,
    share: String,
}

impl ShareAddress {
    /// Checks and keeps a server and share name.
    ///
    /// # Errors
    /// [`InputError::BadServer`] or [`InputError::BadShare`].
    pub fn new(server: &str, share: &str) -> Result<Self, InputError> {
        let server = check_server(server)?;
        let share = check_share(share)?;
        Ok(Self { server, share })
    }

    /// The server's name or address, as typed (an IPv6 address keeps its brackets).
    #[must_use]
    pub fn server(&self) -> &str {
        &self.server
    }

    /// The share's name on the server.
    #[must_use]
    pub fn share(&self) -> &str {
        &self.share
    }

    /// The Windows network path of the share: `\\nas\movies`. An IPv6 address takes the
    /// `ipv6-literal.net` form Windows needs in a network path.
    #[must_use]
    pub fn unc(&self) -> String {
        format!(r"\\{}\{}", unc_host(&self.server), self.share)
    }

    /// The `smb://` address of the share with a trailing `/`, its names percent-encoded.
    #[must_use]
    pub fn smb_url(&self) -> String {
        format!("smb://{}/{}/", self.server, percent_encode(&self.share))
    }
}

impl fmt::Display for ShareAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, r"\\{}\{}", self.server, self.share)
    }
}

/// What the Address field holds: a share, maybe a folder inside it, and the user an `smb://`
/// address may name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub share: ShareAddress,
    /// The folder inside the share, one name per level; empty for the share itself.
    pub folder: Vec<String>,
    /// The user named in `smb://alex@nas/movies`, with a `DOMAIN;` part given as `DOMAIN\`.
    pub user: Option<String>,
}

impl Address {
    /// Reads `\\server\share[\folder…]`, `//server/share[/folder…]` or
    /// `smb://[user@]server/share[/folder…]`. Surrounding spaces and a trailing separator are
    /// ignored; an `smb://` address is percent-decoded.
    ///
    /// # Errors
    /// The [`InputError`] of the first part that is not valid.
    pub fn parse(text: &str) -> Result<Self, InputError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(InputError::EmptyAddress);
        }
        let scheme = text
            .get(..6)
            .filter(|head| head.eq_ignore_ascii_case("smb://"));
        let (rest, decode, user) = if scheme.is_some() {
            let rest = &text[6..];
            let end = rest.find('/').unwrap_or(rest.len());
            let (authority, path) = rest.split_at(end);
            let (user, host) = match authority.rsplit_once('@') {
                Some((info, host)) => (Some(url_user(info)?), host),
                None => (None, authority),
            };
            if host.rsplit_once(':').is_some_and(|(_, port)| {
                !host.ends_with(']') && port.bytes().all(|b| b.is_ascii_digit())
            }) {
                return Err(InputError::PortGiven);
            }
            (format!("{host}{path}"), true, user)
        } else if let Some(rest) = text.strip_prefix(r"\\").or_else(|| text.strip_prefix("//")) {
            (rest.to_owned(), false, None)
        } else {
            return Err(InputError::UnknownForm);
        };

        let rest = rest.trim_end_matches(['\\', '/']);
        let mut parts = rest.split(['\\', '/']);
        let server = parts.next().unwrap_or_default();
        let share = parts.next().ok_or(InputError::MissingShare)?;
        let share = if decode {
            percent_decode(share).ok_or(InputError::BadShare)?
        } else {
            share.to_owned()
        };
        let share = ShareAddress::new(server, &share)?;
        let folder = parts
            .map(|part| {
                let part = if decode {
                    percent_decode(part).ok_or(InputError::BadFolder)?
                } else {
                    part.to_owned()
                };
                check_folder_name(&part).map(|()| part)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            share,
            folder,
            user,
        })
    }

    /// The library folder this address names once the share appears at `root`.
    #[must_use]
    pub fn folder_below(&self, root: &Path) -> PathBuf {
        self.folder
            .iter()
            .fold(root.to_path_buf(), |path, name| path.join(name))
    }
}

/// The user an `smb://` address names (`alex`, `HOME;alex`), as `DOMAIN\name`.
fn url_user(info: &str) -> Result<String, InputError> {
    if info.contains(':') {
        return Err(InputError::PasswordInAddress);
    }
    let info = percent_decode(info).ok_or(InputError::BadUser)?;
    let user = match info.split_once(';') {
        Some((domain, name)) => format!(r"{domain}\{name}"),
        None => info,
    };
    check_user(&user)
}

fn check_server(server: &str) -> Result<String, InputError> {
    if let Some(inner) = server
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        let (address, zone) = match inner.split_once('%') {
            Some((address, zone)) => (address, Some(zone)),
            None => (inner, None),
        };
        let zone_ok = zone
            .is_none_or(|zone| !zone.is_empty() && zone.bytes().all(|b| b.is_ascii_alphanumeric()));
        return if zone_ok && address.parse::<Ipv6Addr>().is_ok() {
            Ok(server.to_owned())
        } else {
            Err(InputError::BadServer)
        };
    }
    let label_ok = |label: &str| {
        (1..=63).contains(&label.len())
            && !label.starts_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    };
    if server.len() <= 253 && server.split('.').all(label_ok) {
        Ok(server.to_owned())
    } else {
        Err(InputError::BadServer)
    }
}

fn check_share(share: &str) -> Result<String, InputError> {
    let refused = |c: char| c.is_control() || r#"\/:*?"<>|"#.contains(c);
    let length = share.chars().count();
    if (1..=80).contains(&length) && !share.chars().any(refused) && share != "." && share != ".." {
        Ok(share.to_owned())
    } else {
        Err(InputError::BadShare)
    }
}

fn check_folder_name(name: &str) -> Result<(), InputError> {
    let ok = !name.is_empty()
        && name != "."
        && name != ".."
        && name.chars().count() <= 255
        && !name.chars().any(char::is_control);
    if ok {
        Ok(())
    } else {
        Err(InputError::BadFolder)
    }
}

/// Checks a user name: printable, at most 256 characters, with an optional `DOMAIN\` part.
/// Surrounding spaces are dropped.
///
/// # Errors
/// [`InputError::EmptyUser`] or [`InputError::BadUser`].
pub fn check_user(user: &str) -> Result<String, InputError> {
    let user = user.trim();
    if user.is_empty() {
        return Err(InputError::EmptyUser);
    }
    let printable = !user.chars().any(char::is_control) && user.chars().count() <= 256;
    let domain_ok = match user.split_once('\\') {
        Some((domain, name)) => !domain.is_empty() && !name.is_empty() && !name.contains('\\'),
        None => true,
    };
    if printable && domain_ok {
        Ok(user.to_owned())
    } else {
        Err(InputError::BadUser)
    }
}

/// The folder `folder` inside a share that appears at `root`, when it is there.
///
/// # Errors
/// [`ShareErrorKind::FolderNotFound`] when it is not a folder that can be read.
pub fn folder_in_share(root: &Path, folder: &[String]) -> Result<PathBuf, ShareError> {
    let path = folder
        .iter()
        .fold(root.to_path_buf(), |path, name| path.join(name));
    if std::fs::read_dir(&path).is_ok() {
        Ok(path)
    } else {
        Err(ShareError::new(
            ShareErrorKind::FolderNotFound,
            format!("{} was not found", path.display()),
        ))
    }
}

/// The host part of a Windows network path: an IPv6 address becomes `fe80--1s4.ipv6-literal.net`.
fn unc_host(server: &str) -> String {
    match server
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        Some(inner) => format!(
            "{}.ipv6-literal.net",
            inner.replace(':', "-").replace('%', "s")
        ),
        None => server.to_owned(),
    }
}

/// Percent-encodes everything but unreserved URL characters.
pub(crate) fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decodes `%XX` sequences; `None` for a broken sequence or text that is not UTF-8 then.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> (String, String, Vec<String>, Option<String>) {
        let address = Address::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        (
            address.share.server().to_owned(),
            address.share.share().to_owned(),
            address.folder,
            address.user,
        )
    }

    fn films(server: &str) -> (String, String, Vec<String>, Option<String>) {
        (server.into(), "movies".into(), vec!["Films".into()], None)
    }

    #[test]
    fn the_three_forms_name_the_same_folder() {
        for text in [
            r"\\nas\movies\Films",
            "//nas/movies/Films",
            "smb://nas/movies/Films",
            "SMB://nas/movies/Films/",
            r"  \\nas\movies\Films\  ",
            r"\\nas/movies\Films",
        ] {
            assert_eq!(parsed(text), films("nas"), "{text}");
        }
    }

    #[test]
    fn the_share_alone_has_no_folder() {
        for text in [
            r"\\nas\movies",
            r"\\nas\movies\",
            "//nas/movies",
            "smb://nas/movies/",
        ] {
            let (server, share, folder, _) = parsed(text);
            assert_eq!((server.as_str(), share.as_str()), ("nas", "movies"));
            assert!(folder.is_empty(), "{text}");
        }
    }

    #[test]
    fn deeper_folders_addresses_and_odd_share_names_are_kept() {
        let (_, _, folder, _) = parsed(r"\\nas\movies\Films\4K HDR");
        assert_eq!(folder, ["Films", "4K HDR"]);
        assert_eq!(
            parsed(r"\\192.168.1.20\movies\Films"),
            films("192.168.1.20")
        );
        assert_eq!(parsed("smb://[fe80::1]/movies/Films"), films("[fe80::1]"));
        assert_eq!(parsed(r"\\[fe80::1%4]\movies\Films"), films("[fe80::1%4]"));
        assert_eq!(
            parsed(r"\\nas.home.lan\movies\Films"),
            films("nas.home.lan")
        );
        assert_eq!(parsed(r"\\my_nas-2\movies\Films"), films("my_nas-2"));
        let (_, share, _, _) = parsed(r"\\nas\Movies & TV$");
        assert_eq!(share, "Movies & TV$");
    }

    #[test]
    fn an_smb_address_is_decoded_and_may_name_the_user() {
        let (_, share, folder, user) = parsed("smb://alex@nas/My%20Movies/Films%20HD");
        assert_eq!(share, "My Movies");
        assert_eq!(folder, ["Films HD"]);
        assert_eq!(user.as_deref(), Some("alex"));
        let (_, _, _, user) = parsed("smb://HOME;alex@nas/movies");
        assert_eq!(user.as_deref(), Some(r"HOME\alex"));
    }

    #[test]
    fn refused_addresses_say_why() {
        for (text, error) in [
            ("", InputError::EmptyAddress),
            ("   ", InputError::EmptyAddress),
            ("nas/movies", InputError::UnknownForm),
            (r"C:\Movies", InputError::UnknownForm),
            ("http://nas/movies", InputError::UnknownForm),
            (r"\nas\movies", InputError::UnknownForm),
            (r"\\nas", InputError::MissingShare),
            (r"\\nas\", InputError::MissingShare),
            ("smb://nas", InputError::MissingShare),
            (r"\\\movies", InputError::BadServer),
            (r"\\..\movies", InputError::BadServer),
            (r"\\nas..lan\movies", InputError::BadServer),
            (r"\\-nas\movies", InputError::BadServer),
            (r"\\nas name\movies", InputError::BadServer),
            (r"\\nas;rm\movies", InputError::BadServer),
            (r"\\[fe80::zz]\movies", InputError::BadServer),
            (r"\\[fe80::1%]\movies", InputError::BadServer),
            ("smb://nas:445/movies", InputError::PortGiven),
            (
                "smb://alex:secret@nas/movies",
                InputError::PasswordInAddress,
            ),
            (r"\\nas\..", InputError::BadShare),
            (r"\\nas\.", InputError::BadShare),
            (r"\\nas\mov:ies", InputError::BadShare),
            (r"\\nas\mov*ies", InputError::BadShare),
            ("\\\\nas\\mov\u{7}ies", InputError::BadShare),
            ("smb://nas/mov%2Fies", InputError::BadShare),
            ("smb://nas/mov%zzies", InputError::BadShare),
            (r"\\nas\movies\..\other", InputError::BadFolder),
            (r"\\nas\movies\.\Films", InputError::BadFolder),
            (r"\\nas\movies\\Films", InputError::BadFolder),
            ("\\\\nas\\movies\\Fi\nlms", InputError::BadFolder),
            ("smb://nas/movies/%2E%2E", InputError::BadFolder),
        ] {
            assert_eq!(Address::parse(text), Err(error), "{text:?}");
        }
        let long = format!(r"\\nas\{}", "s".repeat(81));
        assert_eq!(Address::parse(&long), Err(InputError::BadShare));
        assert!(Address::parse(&format!(r"\\nas\{}", "s".repeat(80))).is_ok());
        let long_label = format!(r"\\{}\movies", "n".repeat(64));
        assert_eq!(Address::parse(&long_label), Err(InputError::BadServer));
    }

    #[test]
    fn user_names_are_checked() {
        assert_eq!(check_user(" alex ").as_deref(), Ok("alex"));
        assert_eq!(check_user(r"HOME\alex").as_deref(), Ok(r"HOME\alex"));
        assert_eq!(check_user("alex@home.lan").as_deref(), Ok("alex@home.lan"));
        assert_eq!(check_user("Ålex Grün").as_deref(), Ok("Ålex Grün"));
        for (user, error) in [
            ("", InputError::EmptyUser),
            ("  ", InputError::EmptyUser),
            (r"\alex", InputError::BadUser),
            (r"HOME\", InputError::BadUser),
            (r"A\B\alex", InputError::BadUser),
            ("al\nex", InputError::BadUser),
            ("al\tex", InputError::BadUser),
        ] {
            assert_eq!(check_user(user), Err(error), "{user:?}");
        }
        assert_eq!(check_user(&"u".repeat(257)), Err(InputError::BadUser));
    }

    #[test]
    fn a_share_has_its_network_path_and_url() {
        let share = ShareAddress::new("nas", "My Movies").unwrap();
        assert_eq!(share.unc(), r"\\nas\My Movies");
        assert_eq!(share.to_string(), r"\\nas\My Movies");
        assert_eq!(share.smb_url(), "smb://nas/My%20Movies/");
        let v6 = ShareAddress::new("[fe80::1%4]", "movies").unwrap();
        assert_eq!(v6.unc(), r"\\fe80--1s4.ipv6-literal.net\movies");
    }

    #[test]
    fn the_folder_is_found_below_the_share_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Films").join("4K")).unwrap();
        let address = Address::parse(r"\\nas\movies\Films\4K").unwrap();

        let found = folder_in_share(dir.path(), &address.folder).unwrap();
        assert_eq!(found, address.folder_below(dir.path()));
        assert_eq!(found, dir.path().join("Films").join("4K"));
        let missing = folder_in_share(dir.path(), &["Series".to_owned()]).unwrap_err();
        assert_eq!(missing.kind(), ShareErrorKind::FolderNotFound);
        assert_eq!(folder_in_share(dir.path(), &[]).unwrap(), dir.path());
    }
}
