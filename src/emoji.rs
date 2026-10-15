use std::sync::OnceLock;

pub struct Emoji {
    pub symbol: &'static str,
    pub name: &'static str,
    pub category: String,
    pub keywords: String,
}

// A compact everyday selection, in default browsing order. The picker promotes
// favorites and the user's actual usage ahead of this order.
const EVERYDAY: &[&str] = &[
    "😂",
    "❤️",
    "🤣",
    "👍",
    "😊",
    "🙏",
    "🥰",
    "😍",
    "😘",
    "😁",
    "😀",
    "😄",
    "😃",
    "😅",
    "🥹",
    "😎",
    "🤔",
    "😉",
    "🙌",
    "👏",
    "🎉",
    "🔥",
    "✨",
    "✅",
    "💯",
    "💪",
    "👌",
    "👋",
    "🤝",
    "🤗",
    "🥳",
    "😢",
    "😭",
    "😔",
    "😴",
    "😬",
    "😳",
    "🙄",
    "😮",
    "🤦",
    "🤷",
    "😡",
    "🤯",
    "🫡",
    "👀",
    "💖",
    "💕",
    "💙",
    "💚",
    "💛",
    "💜",
    "🧡",
    "🤍",
    "💔",
    "❌",
    "⚠️",
    "⭐",
    "🌟",
    "🎂",
    "🎁",
    "☕",
    "🍕",
    "🍔",
    "🍎",
    "🎵",
    "🎧",
    "📸",
    "🐶",
    "🐱",
    "🐧",
    "🌞",
    "🌈",
    "🌻",
    "🚀",
    "💻",
    "📱",
    "💡",
    "📝",
    "📅",
    "⏰",
    "📌",
    "📍",
    "🏠",
    "⚡",
    "👎",
    "👩‍💻",
    "👨‍💻",
    "🇦🇺",
    "👍🏻",
    "👍🏼",
    "👍🏽",
    "👍🏾",
    "👍🏿",
];

/// Use Unicode's names and exact sequences for the everyday selection.
pub fn catalog() -> &'static [Emoji] {
    static CATALOG: OnceLock<Vec<Emoji>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut entries = Vec::new();
        let mut group = "";
        let mut subgroup = "";
        for line in include_str!("../assets/emoji-test.txt").lines() {
            if let Some(value) = line.strip_prefix("# group: ") {
                group = value;
            } else if let Some(value) = line.strip_prefix("# subgroup: ") {
                subgroup = value;
            } else if let Some((sequence, description)) = line.split_once('#') {
                let Some((_, status)) = sequence.split_once(';') else {
                    continue;
                };
                if !matches!(status.trim(), "fully-qualified" | "component") {
                    continue;
                }
                // Description is: glyph, emoji version, then the full English name.
                let mut parts = description.trim().splitn(3, ' ');
                let symbol = parts.next().expect("Unicode glyph");
                if !EVERYDAY.contains(&symbol) {
                    continue;
                }
                let _version = parts.next().expect("Unicode emoji version");
                let name = parts.next().expect("Unicode emoji name");
                let category = format!("{group} · {}", subgroup.replace('-', " "));
                let aliases = match symbol {
                    "😀" => "happy smile",
                    "😂" => "laughing tears joy",
                    "🥰" => "love hearts",
                    "😎" => "cool sunglasses",
                    "👍" => "thumbs up yes",
                    "👎" => "thumbs down no",
                    "👋" => "wave hello",
                    "🙏" => "thanks please pray",
                    "🎉" => "celebrate party",
                    "🔥" => "fire hot",
                    "❤️" => "heart love",
                    "✅" => "check done tick",
                    "❌" => "cross cancel",
                    "⚡" => "lightning fast",
                    "🚀" => "rocket launch",
                    "💻" => "computer laptop",
                    "🐧" => "penguin linux",
                    "☕" => "coffee tea",
                    _ => "",
                };
                entries.push(Emoji {
                    symbol,
                    name,
                    keywords: format!("{aliases} {name} {symbol} {category}"),
                    category,
                });
            }
        }
        entries.sort_by_key(|entry| {
            EVERYDAY
                .iter()
                .position(|symbol| *symbol == entry.symbol)
                .unwrap()
        });
        // Preserve useful non-emoji symbols from the original picker.
        for (symbol, name) in [
            ("→", "right arrow"),
            ("←", "left arrow"),
            ("✓", "tick check"),
            ("©", "copyright"),
            ("°", "degree"),
        ] {
            entries.push(Emoji {
                symbol,
                name,
                category: "Text symbols".into(),
                keywords: format!("{name} {symbol} text symbols"),
            });
        }
        entries
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn everyday_selection_preserves_sequences_and_excludes_unwanted_emoji() {
        let entries = catalog();
        assert_eq!(entries.len(), EVERYDAY.len() + 5);
        assert!(entries.len() < 100);
        assert!(!entries.iter().any(|entry| entry.symbol == "🍆"));
        assert!(!entries.iter().any(|entry| entry.symbol == "🫪"));
        assert_eq!(
            entries
                .iter()
                .map(|e| e.symbol)
                .collect::<HashSet<_>>()
                .len(),
            entries.len()
        );
        for (symbol, name) in [
            ("🐧", "penguin"),
            ("👍🏿", "thumbs up: dark skin tone"),
            ("👩‍💻", "woman technologist"),
            ("🇦🇺", "flag: Australia"),
            ("😀", "grinning face"),
            ("❤️", "red heart"),
        ] {
            assert!(
                entries.iter().any(|e| e.symbol == symbol && e.name == name),
                "Missing {name}"
            );
        }
        assert!(
            entries
                .iter()
                .any(|e| e.symbol == "🐧" && e.keywords.contains("linux"))
        );
    }
}
