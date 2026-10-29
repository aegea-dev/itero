//! Reading a key from a property list in XML, as Info.plist and launchd's
//! agents are written: the one string a key gives.

/// The string a plist gives `key`, its XML entities read.
pub fn string(text: &str, key: &str) -> Option<String> {
    let after = &text[text.find(&format!("<key>{key}</key>"))?..];
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")? + start;
    Some(
        after[start..end]
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&"),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_key_gives_its_string_with_entities_read() {
        let text = "<dict>\n  <key>CFBundleVersion</key>\n  <string>21</string>\n  <key>Yol</key>\n  <string>/Users/biri/iş &amp; ev</string>\n</dict>";
        assert_eq!(
            super::string(text, "CFBundleVersion").as_deref(),
            Some("21")
        );
        assert_eq!(
            super::string(text, "Yol").as_deref(),
            Some("/Users/biri/iş & ev")
        );
        assert_eq!(super::string(text, "Yok"), None);
    }
}
