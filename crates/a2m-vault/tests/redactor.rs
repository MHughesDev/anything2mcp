//! The redactor's invariants: a registered secret never survives `redact` in
//! any form text carries it in, and `contains_secret` finds exactly what
//! `redact` would mask.

use std::sync::Arc;

use a2m_core::{REDACTED, Redact, Secret};
use a2m_vault::Redactor;

/// A secret with every kind of character an encoding treats specially: a
/// space, a quote, a backslash, a slash, a plus, a percent sign, and
/// characters outside ASCII, one of them outside the basic plane.
const SECRET: &str = "pa ss\"w\\o/r+d%é😀Zq9";

fn redactor(secrets: &[&str]) -> Redactor {
    let redactor = Redactor::new();
    for secret in secrets {
        redactor.register(&Secret::new(*secret));
    }
    redactor
}

/// Base64 written here, apart from the crate, so the test does not check the
/// redactor against its own arithmetic.
fn base64(bytes: &[u8], url_safe: bool, pad: bool) -> String {
    let alphabet: Vec<char> = if url_safe {
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
    } else {
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    }
    .chars()
    .collect();
    let mut out = String::new();
    for group in bytes.chunks(3) {
        let n = group
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, byte)| n | (u32::from(*byte) << (16 - 8 * i)));
        for i in 0..=group.len() {
            out.push(alphabet[((n >> (18 - 6 * i)) & 63) as usize]);
        }
        if pad {
            out.push_str(&"=".repeat(3 - group.len()));
        }
    }
    out
}

fn percent(text: &str, keep: impl Fn(u8) -> bool, upper: bool) -> String {
    text.bytes()
        .map(|byte| match byte {
            byte if keep(byte) => char::from(byte).to_string(),
            byte if upper => format!("%{byte:02X}"),
            byte => format!("%{byte:02x}"),
        })
        .collect()
}

fn unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-._~".contains(&byte)
}

/// JSON with everything outside ASCII written as `\u` escapes, as some
/// serializers do, and the slash escaped, as others do.
fn json_ascii(text: &str) -> String {
    let mut out = String::new();
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            character if character.is_ascii() => out.push(character),
            character => {
                let mut units = [0u16; 2];
                for unit in character.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out
}

/// The forms of [`SECRET`] that text commonly carries.
fn encoded_forms() -> Vec<(&'static str, String)> {
    let json = serde_json::json!(SECRET).to_string();
    let json = json.trim_matches('"').to_owned();
    let twice = serde_json::json!(json).to_string();
    let every_character: String = SECRET
        .encode_utf16()
        .map(|unit| format!("\\u{unit:04X}"))
        .collect();
    let form: String =
        percent(SECRET, |byte| unreserved(byte) || byte == b' ', true).replace(' ', "+");
    vec![
        ("exact", SECRET.to_owned()),
        ("percent, upper case", percent(SECRET, unreserved, true)),
        ("percent, lower case", percent(SECRET, unreserved, false)),
        ("percent, every byte", percent(SECRET, |_| false, true)),
        (
            "percent, as a script encodes a component",
            percent(
                SECRET,
                |byte| unreserved(byte) || b"!*'()".contains(&byte),
                true,
            ),
        ),
        ("form submission", form),
        (
            "percent, twice",
            percent(&percent(SECRET, unreserved, true), unreserved, true),
        ),
        ("json", json.clone()),
        ("json, ascii only", json_ascii(SECRET)),
        ("json, every character escaped", every_character),
        ("json, twice", twice.trim_matches('"').to_owned()),
        ("json inside percent", percent(&json, unreserved, true)),
        ("base64", base64(SECRET.as_bytes(), false, true)),
        (
            "base64, url-safe, no padding",
            base64(SECRET.as_bytes(), true, false),
        ),
        (
            "base64 inside percent",
            percent(&base64(SECRET.as_bytes(), false, true), unreserved, true),
        ),
        (
            "base64 inside json with the slash escaped",
            base64(SECRET.as_bytes(), false, true).replace('/', "\\/"),
        ),
    ]
}

#[test]
fn a_registered_secret_never_survives_in_any_encoded_form() {
    let redactor = redactor(&[SECRET]);
    for (name, form) in encoded_forms() {
        let text = format!("before {form} after");
        assert!(redactor.contains_secret(&text), "{name}: not found");
        let redacted = redactor.redact(&text);
        assert_eq!(redacted, format!("before {REDACTED} after"), "{name}");
        assert!(!redactor.contains_secret(&redacted), "{name}: still found");
    }
}

#[test]
fn a_secret_inside_a_larger_base64_value_is_masked_at_every_alignment() {
    let secret = "s3cr3t-Value_9";
    let redactor = redactor(&[secret]);
    for url_safe in [false, true] {
        for before in ["", "u", "us", "user:", "a-user:"] {
            for after in ["", "x", "xy", "xyz"] {
                let plain = format!("{before}{secret}{after}");
                let encoded = base64(plain.as_bytes(), url_safe, !url_safe);
                let text = format!("Authorization: Basic {encoded} end");
                let redacted = redactor.redact(&text);
                let case = format!("{before:?} {after:?} {url_safe}: {redacted}");

                // Only characters made of no bit of the secret may remain.
                let first = before.len() * 8 / 6;
                let last = ((before.len() + secret.len()) * 8).div_ceil(6);
                let rest = redacted.strip_prefix("Authorization: Basic ").expect(&case);
                let rest = rest.strip_suffix(" end").expect(&case);
                let (kept_before, kept_after) = rest.split_once(REDACTED).expect(&case);
                assert!(encoded.starts_with(kept_before), "{case}");
                assert!(encoded.ends_with(kept_after), "{case}");
                assert!(kept_before.len() <= first, "{case}");
                let unpadded = encoded.trim_end_matches('=').len();
                assert!(
                    kept_after.trim_end_matches('=').len() <= unpadded - last,
                    "{case}"
                );
                assert!(redactor.contains_secret(&text), "{case}");
                assert!(!redactor.contains_secret(&redacted), "{case}");
            }
        }
    }
}

#[test]
fn contains_secret_finds_exactly_what_redact_would_mask() {
    let redactor = redactor(&[SECRET, "tok_3x4mpl3", "4321"]);
    let mut texts: Vec<String> = encoded_forms().into_iter().map(|(_, form)| form).collect();
    texts.extend(
        [
            "",
            "nothing to see",
            "100% sure \\ not an escape %zz \\u12",
            "the token is tok_3x4mpl3.",
            "pin 4321 and 54321",
            "already [redacted] here",
            "dG9rXzN4NG1wbDM=",
            "tok_3x4mpl",
            "é😀 unrelated text outside ASCII",
        ]
        .map(str::to_owned),
    );
    for text in texts {
        let redacted = redactor.redact(&text);
        assert_eq!(redactor.contains_secret(&text), redacted != text, "{text}");
        assert!(!redactor.contains_secret(&redacted), "{text}");
        assert_eq!(redactor.redact(&redacted), redacted, "{text}");
    }
}

#[test]
fn text_with_no_secret_comes_back_unchanged() {
    let none = Redactor::new();
    assert_eq!(none.redact("anything at all"), "anything at all");
    assert!(!none.contains_secret("anything at all"));

    let redactor = redactor(&[SECRET]);
    let text =
        "{\"path\":\"C:\\\\data\\\\file\",\"rate\":\"50%\",\"note\":\"caf\\u00e9 \\ud83d\\ude00\"}";
    assert_eq!(redactor.redact(text), text);
    assert!(!redactor.contains_secret(text));
}

#[test]
fn a_short_secret_is_registered_and_masked_and_an_empty_one_is_ignored() {
    let redactor = redactor(&["", "4321", "k"]);
    let health = redactor.health();
    assert_eq!((health.registered, health.short), (2, 2));
    assert!(!health.over_bound());

    assert_eq!(redactor.redact("code 4321."), format!("code {REDACTED}."));
    // Masking too much is the safer error: the one-character secret masks
    // that character wherever it stands.
    assert_eq!(redactor.redact("ok"), format!("o{REDACTED}"));
    // A short secret is masked when it is percent-encoded or JSON-escaped,
    // and when it is base64-encoded on its own.
    assert_eq!(redactor.redact("%34%33%32%31"), REDACTED);
    assert_eq!(redactor.redact("\\u0034321"), REDACTED);
    assert_eq!(redactor.redact("x=NDMyMQ== y"), format!("x={REDACTED} y"));
    assert_eq!(redactor.redact(""), "");
}

#[test]
fn secrets_that_overlap_or_touch_are_masked_whole() {
    let redactor = redactor(&["abcd-1234", "1234-wxyz", "wxyz!"]);
    assert_eq!(
        redactor.redact("<abcd-1234-wxyz!>"),
        format!("<{REDACTED}>")
    );
    assert_eq!(
        redactor.redact("abcd-1234 and 1234-wxyz"),
        format!("{REDACTED} and {REDACTED}")
    );
}

#[test]
fn the_mask_itself_never_hides_or_uncovers_a_secret() {
    // One secret is part of the mask's own text; another only appears once
    // the first has been replaced by a mask.
    let redactor = redactor(&["red", "long-secret-value", "d]tail"]);
    for text in [
        "long-secret-valuetail",
        "a red door and long-secret-value",
        "[redacted] was here, and red too",
    ] {
        let redacted = redactor.redact(text);
        assert!(!redactor.contains_secret(&redacted), "{text} -> {redacted}");
        assert!(!redacted.contains("long-secret-value"), "{redacted}");
        assert!(!redacted.contains("d]tail"), "{redacted}");
        assert!(
            !redacted.replace(REDACTED, "").contains("red"),
            "{redacted}"
        );
    }
    assert_eq!(redactor.redact("[redacted]"), "[redacted]");
}

#[test]
fn a_clone_shares_the_registered_set_across_threads() {
    let redactor = Redactor::new();
    let shared: Arc<dyn Redact> = Arc::new(redactor.clone());
    let writer = redactor.clone();
    std::thread::spawn(move || writer.register(&Secret::new("tok_3x4mpl3")))
        .join()
        .unwrap();
    let reader = std::thread::spawn(move || shared.redact("token tok_3x4mpl3"));
    assert_eq!(reader.join().unwrap(), format!("token {REDACTED}"));
    assert_eq!(redactor.health().registered, 1);

    // Registering the same secret again changes nothing.
    redactor.register(&Secret::new("tok_3x4mpl3"));
    assert_eq!(redactor.health().registered, 1);
}

#[test]
fn the_bound_is_reported_and_nothing_is_dropped_to_stay_under_it() {
    let redactor = Redactor::new();
    let secret = |n: usize| format!("secret-number-{n:05}-end");
    for n in 0..=Redactor::BOUND {
        redactor.register(&Secret::new(secret(n)));
    }
    let health = redactor.health();
    assert_eq!(health.registered, Redactor::BOUND + 1);
    assert!(health.over_bound());

    // The first and the last are both still masked, in a large clean text.
    let filler = "a line of ordinary text with nothing secret in it\n".repeat(2000);
    assert_eq!(redactor.redact(&filler), filler);
    let text = format!("{filler}{} {filler}{}", secret(0), secret(Redactor::BOUND));
    assert_eq!(
        redactor.redact(&text),
        format!("{filler}{REDACTED} {filler}{REDACTED}")
    );
}

#[test]
fn nothing_the_redactor_prints_holds_a_secret() {
    let redactor = redactor(&[SECRET, "tok_3x4mpl3"]);
    let shown = format!("{redactor:?} {:?}", redactor.health());
    assert!(!shown.contains("tok_3x4mpl3"), "{shown}");
    assert!(!shown.contains("pa ss"), "{shown}");
}

/// A token as an identity service issues one: long, and made of characters
/// no encoding escapes.
const TOKEN: &str = "example-token-Zq9mV7tK2xLw4Nc8Rb1Hd6Jf3Gs5Py0AuEiOo-_~.end";

/// `text` broken into lines of `width` characters, as mail and key files
/// wrap base64.
fn wrapped(text: &str, width: usize, line_break: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    characters
        .chunks(width)
        .map(|line| line.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(line_break)
}

/// Audit finding V-2, letter case. Systems print hexadecimal in either
/// case, so a secret written in it is found in any.
#[test]
fn a_hexadecimal_secret_is_masked_in_any_letter_case() {
    for secret in [
        "4f8a0c2e9b7d1a3f5e6c8b0d2f4a6c8e",
        "4F8A0C2E-9B7D-1A3F-5E6C-8B0D2F4A6C8E",
        "0a:1b:2c:3d:4e:5f",
    ] {
        let redactor = redactor(&[secret]);
        let mixed: String = secret
            .chars()
            .enumerate()
            .map(|(at, c)| {
                if at % 3 == 0 {
                    c.to_ascii_uppercase()
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect();
        for spelling in [
            secret.to_owned(),
            secret.to_ascii_uppercase(),
            secret.to_ascii_lowercase(),
            mixed,
        ] {
            let text = format!("key={spelling};");
            assert!(redactor.contains_secret(&text), "{spelling}");
            assert_eq!(redactor.redact(&text), format!("key={REDACTED};"));
        }
    }

    // A secret of any other shape is matched as it is written, and in
    // another case only when it was registered so.
    let exact = redactor(&[TOKEN]);
    assert!(!exact.contains_secret(&TOKEN.to_ascii_uppercase()));
    let folded = Redactor::new();
    folded.register_folded(&Secret::new(TOKEN));
    for spelling in [
        TOKEN.to_owned(),
        TOKEN.to_ascii_uppercase(),
        TOKEN.to_ascii_lowercase(),
    ] {
        assert_eq!(
            folded.redact(&format!("<{spelling}>")),
            format!("<{REDACTED}>")
        );
    }
    // Registering it both ways counts it once, in either order.
    folded.register(&Secret::new(TOKEN));
    exact.register_folded(&Secret::new(TOKEN));
    assert_eq!(folded.health().registered, 1);
    assert_eq!(exact.health().registered, 1);
    assert!(exact.contains_secret(&TOKEN.to_ascii_uppercase()));
}

/// The hexadecimal writing of a secret's bytes, as a frame capture or a
/// dump shows them.
#[test]
fn a_secret_written_out_in_hexadecimal_is_masked() {
    let redactor = redactor(&[SECRET, TOKEN]);
    for secret in [SECRET, TOKEN] {
        let lower: String = secret.bytes().map(|byte| format!("{byte:02x}")).collect();
        let upper = lower.to_ascii_uppercase();
        for spelling in [lower, upper] {
            let text = format!("{{\"frame\":\"0102{spelling}0304\"}}");
            assert!(redactor.contains_secret(&text));
            assert_eq!(
                redactor.redact(&text),
                format!("{{\"frame\":\"0102{REDACTED}0304\"}}")
            );
        }
    }
}

/// Audit finding V-2, base64 with line breaks. Mail wraps base64 at 76
/// characters and key files at 64; a secret longer than a line is found
/// across the breaks, also where the breaks are written as escapes.
#[test]
fn a_secret_in_base64_broken_into_lines_is_masked() {
    let redactor = redactor(&[SECRET, TOKEN]);
    let value = format!("first part {SECRET} middle {TOKEN} last part, long enough to wrap");
    for (url_safe, pad) in [(false, true), (true, false)] {
        let encoded = base64(value.as_bytes(), url_safe, pad);
        for width in [76, 64, 20, 7] {
            for line_break in ["\n", "\r\n", "\n  ", "\r\n\t"] {
                let text = format!("body:\n{}\n--", wrapped(&encoded, width, line_break));
                assert!(redactor.contains_secret(&text), "{width} {line_break:?}");
                let clean = redactor.redact(&text);
                assert!(!redactor.contains_secret(&clean), "{clean}");
                assert!(clean.starts_with("body:\n") && clean.ends_with("\n--"));
            }
            // In a JSON string the break is written as an escape, and in a
            // query it is percent-encoded.
            let in_json = wrapped(&encoded, width, "\\n");
            assert!(redactor.contains_secret(&in_json), "json {width}");
            assert!(!redactor.contains_secret(&redactor.redact(&in_json)));
            let in_query = percent(&wrapped(&encoded, width, "\r\n"), unreserved, true);
            assert!(redactor.contains_secret(&in_query), "query {width}");
            assert!(!redactor.contains_secret(&redactor.redact(&in_query)));
        }
    }
    // Words on two lines are not a secret because a line break joins them.
    redactor.register(&Secret::new("word1word2"));
    assert!(!redactor.contains_secret("word1\nword2"));
}

/// Audit finding V-2, character references. A page's source writes `&`,
/// `<`, `>`, and the quotes as references, and may write any character as
/// a number.
#[test]
fn a_secret_written_with_character_references_is_masked() {
    let secret = "p&ss<w>o\"r'd\u{e9}-Zq9";
    let redactor = redactor(&[secret]);
    let named = secret
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
    let decimal: String = secret
        .chars()
        .map(|c| format!("&#{};", u32::from(c)))
        .collect();
    let hexadecimal: String = secret
        .chars()
        .map(|c| format!("&#x{:X};", u32::from(c)))
        .collect();
    let mixed = secret.replace('&', "&#38;").replace('\'', "&#x27;");
    for spelling in [named, decimal, hexadecimal, mixed] {
        let text = format!("<input value=\"{spelling}\">");
        assert!(redactor.contains_secret(&text), "{spelling}");
        assert_eq!(
            redactor.redact(&text),
            format!("<input value=\"{REDACTED}\">")
        );
    }
    // A reference that is not one stays as it is.
    let untouched = "a &amp b &; &#; &#xZZ; &unknown; a=1&b=2";
    assert_eq!(redactor.redact(untouched), untouched);
}

/// Audit finding V-5 led the registered set to be kept as the forms
/// themselves. A secret that equals a form of another secret is still a
/// secret of its own, with all its own forms.
#[test]
fn a_secret_equal_to_a_form_of_another_is_registered_whole() {
    let redactor = redactor(&["first second"]);
    // The form-submission spelling of the first secret.
    redactor.register(&Secret::new("first+second"));
    assert_eq!(redactor.health().registered, 2);
    let encoded = base64(b"first+second", false, true);
    assert!(redactor.contains_secret(&encoded));
    redactor.register(&Secret::new("first second"));
    redactor.register(&Secret::new("first+second"));
    assert_eq!(redactor.health().registered, 2);
}
