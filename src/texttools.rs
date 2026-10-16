use crate::boosters::{self, Entry, Operation};
use gtk::glib;
use std::{fs, process::Command, time::Duration};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Query {
    Uuid,
    Hash(String),
    Base64(String),
    Tldr(String),
}
impl Query {
    pub fn parse(head: &str, tail: &str) -> Option<Self> {
        Some(match head {
            "uuid" => Self::Uuid,
            "hash" => Self::Hash(tail.into()),
            "base64" => Self::Base64(tail.into()),
            "tldr" | "cheat" => Self::Tldr(tail.into()),
            _ => return None,
        })
    }
}
fn copy(title: String, detail: &str, badge: &'static str) -> Entry {
    let display: String = title
        .chars()
        .take(160)
        .map(|c| match c {
            '\n' | '\r' => '⏎',
            '\t' => '⇥',
            c if c.is_control() => '�',
            c => c,
        })
        .collect();
    Entry {
        operation: Some(Operation::Copy(title.clone())),
        title: if title.chars().count() > 160 {
            format!("{display}…")
        } else {
            display
        },
        detail: detail.into(),
        badge,
    }
}
fn input(text: &str) -> Result<String, String> {
    if text.len() > 65536 {
        return Err("Text is limited to 64 KiB".into());
    }
    if text.starts_with('"') {
        serde_json::from_str(text)
            .map_err(|_| "Use a valid JSON quoted string, for example \"text\\nmore\"".into())
    } else {
        Ok(text.into())
    }
}
pub fn fetch(query: &Query) -> Result<Vec<Entry>, String> {
    Ok(match query {
        Query::Uuid => vec![copy(
            glib::uuid_string_random().into(),
            "Random UUID v4 · Enter copies · Ctrl+R generates another",
            "UUID",
        )],
        Query::Hash(text) => {
            if text.is_empty() {
                return Ok(vec![boosters::hint(
                    "Hash text",
                    "Type hash mytext · SHA-256 by default · hash sha512 text or hash md5 text",
                )]);
            }
            let (algorithm, data, badge) = match text.split_once(' ') {
                Some(("sha256", rest)) => (glib::ChecksumType::Sha256, rest, "SHA-256"),
                Some(("sha512", rest)) => (glib::ChecksumType::Sha512, rest, "SHA-512"),
                Some(("sha1", rest)) => (glib::ChecksumType::Sha1, rest, "SHA-1"),
                Some(("md5", rest)) => (glib::ChecksumType::Md5, rest, "MD5"),
                _ => (glib::ChecksumType::Sha256, text.as_str(), "SHA-256"),
            };
            let text = input(data)?;
            vec![copy(
                glib::compute_checksum_for_data(algorithm, text.as_bytes())
                    .ok_or("Hash algorithm unavailable")?
                    .into(),
                &format!("{badge} · UTF-8 · {} bytes · Enter copies", text.len()),
                badge,
            )]
        }
        Query::Base64(text) => {
            let (mode, text) = text.split_once(' ').unwrap_or((text, ""));
            if !["encode", "decode"].contains(&mode) || text.is_empty() {
                return Ok(vec![boosters::hint(
                    "Base64 encode / decode",
                    "Type base64 encode mytext or base64 decode bXl0ZXh0 · quote text to preserve whitespace",
                )]);
            }
            let text = input(text)?;
            if mode == "encode" {
                vec![copy(
                    glib::base64_encode(text.as_bytes()).into(),
                    "Base64 of UTF-8 text · Enter copies",
                    "Base64",
                )]
            } else {
                let compact: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
                if !compact.is_ascii()
                    || !compact.len().is_multiple_of(4)
                    || compact.as_bytes().contains(&0)
                {
                    return Err("Invalid Base64 · use standard padded Base64".into());
                }
                let bytes = glib::base64_decode(&compact);
                if glib::base64_encode(&bytes).as_str() != compact {
                    return Err("Invalid Base64 · use standard padded Base64".into());
                }
                let value = String::from_utf8(bytes)
                    .map_err(|_| "Decoded bytes are binary, not UTF-8 text")?;
                if value.contains('\0') {
                    return Err(
                        "Decoded text contains a NUL byte and cannot be copied as text".into(),
                    );
                }
                vec![copy(value, "Decoded UTF-8 text · Enter copies", "Base64")]
            }
        }
        Query::Tldr(command) => return cheat(command),
    })
}
const TAR: &str = "tar — common archive commands\n\nCreate a compressed archive:\n  tar -czf archive.tar.gz directory/\n\nList an archive before extracting:\n  tar -tf archive.tar.gz\n\nExtract into an existing directory:\n  tar -xf archive.tar.gz -C destination/\n\nCreate an uncompressed archive:\n  tar -cf archive.tar file1 file2\n\nExtract one member:\n  tar -xf archive.tar path/inside/archive\n\nInspect verbose file details:\n  tar -tvf archive.tar\n\nSpotlight's bundled cheat sheet. Commands are examples; they are never run by Spotlight.";
fn cheat(command: &str) -> Result<Vec<Entry>, String> {
    if command.is_empty() {
        return Ok(vec![boosters::hint(
            "Command cheat sheets",
            "Type tldr tar or tldr git commit · read examples inline",
        )]);
    }
    let page = command.split_whitespace().collect::<Vec<_>>().join("-");
    if page.len() > 100
        || page.starts_with('-')
        || !page
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err("Use a command name, for example tldr tar or tldr git commit".into());
    }
    if page == "tar" {
        return Ok(preview(command, TAR.into(), "Bundled · works offline"));
    }
    let cache = glib::user_cache_dir()
        .join("spotlight/tldr")
        .join(format!("{page}.json"));
    if let Ok(meta) = fs::metadata(&cache)
        && meta.len() < 131072
        && meta
            .modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(604800))
        && let Ok(text) = fs::read_to_string(&cache)
        && let Ok((source, body)) = serde_json::from_str::<(String, String)>(&text)
    {
        return Ok(preview(
            command,
            format!("{body}\n\nSource: {source}\ntldr-pages contributors · CC BY 4.0"),
            "Cached · tldr-pages",
        ));
    }
    for platform in ["linux", "common"] {
        let source = format!(
            "https://raw.githubusercontent.com/tldr-pages/tldr/main/pages/{platform}/{page}.md"
        );
        if let Ok(body) = boosters::run(
            Command::new("curl").args([
                "--fail",
                "--silent",
                "--show-error",
                "--connect-timeout",
                "3",
                "--max-time",
                "5",
                "--max-filesize",
                "65536",
                &source,
            ]),
            6,
        ) {
            if !body.starts_with("# ") {
                continue;
            }
            let _ = save_cache(&cache, &serde_json::to_string(&(&source, &body)).unwrap());
            return Ok(preview(
                command,
                format!("{body}\n\nSource: {source}\ntldr-pages contributors · CC BY 4.0"),
                "tldr-pages · CC BY 4.0",
            ));
        }
    }
    Err(format!(
        "No cheat sheet available for {command} · check the command and internet connection"
    ))
}
fn save_cache(path: &std::path::Path, text: &str) -> Result<(), std::io::Error> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    let tmp = path.with_extension("tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(text.as_bytes())?;
    fs::rename(tmp, path)
}
fn preview(command: &str, body: String, detail: &str) -> Vec<Entry> {
    let mut rows = vec![Entry {
        title: format!("Cheat sheet · {command}"),
        detail: format!("{detail} · Enter opens the full sheet inline"),
        badge: "Read",
        operation: Some(Operation::Preview {
            title: format!("tldr {command}"),
            text: body.clone(),
            open: None,
        }),
    }];
    let mut description = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("tar ") || (trimmed.starts_with('`') && trimmed.ends_with('`')) {
            let example = trimmed.trim_matches('`').to_string();
            rows.push(copy(example, &description, "Example"));
            if rows.len() >= 14 {
                break;
            }
        } else if !trimmed.is_empty() && !trimmed.starts_with(['#', '>']) {
            description = trimmed
                .trim_start_matches("- ")
                .trim_end_matches(':')
                .into();
        }
    }
    rows
}
#[cfg(test)]
mod tests {
    use super::*;
    fn value(q: Query) -> String {
        fetch(&q).unwrap().remove(0).title
    }
    #[test]
    fn standard_vectors_and_strict_base64() {
        assert_eq!(
            value(Query::Hash("abc".into())),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            value(Query::Hash("md5 abc".into())),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            value(Query::Hash("\"\"".into())),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(value(Query::Base64("encode café".into())), "Y2Fmw6k=");
        assert_eq!(value(Query::Base64("decode Y2Fmw6k=".into())), "café");
        for bad in [
            "decode a!",
            "decode YQ=",
            "decode YR==",
            "decode /w==",
            "decode AA==",
        ] {
            assert!(fetch(&Query::Base64(bad.into())).is_err(), "{bad}");
        }
        assert_eq!(value(Query::Base64("encode \" a \"".into())), "IGEg");
    }
    #[test]
    fn random_uuid_and_offline_sheet() {
        let a = value(Query::Uuid);
        let b = value(Query::Uuid);
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
        assert!("89ab".contains(&a[19..20]));
        assert!(matches!(
            fetch(&Query::Tldr("tar".into())).unwrap()[0].operation,
            Some(Operation::Preview { .. })
        ));
        assert!(fetch(&Query::Tldr("../etc/passwd".into())).is_err());
    }
}
