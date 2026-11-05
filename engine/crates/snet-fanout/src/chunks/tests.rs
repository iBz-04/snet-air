use super::{chunk_size, escaped_len, split};

fn rejoined(text: &str, size: usize) -> String {
    let chunks = split(text, size);
    for range in &chunks {
        assert!(escaped_len(&text[range.clone()]) <= size, "{range:?}");
    }
    chunks.iter().map(|range| &text[range.clone()]).collect()
}

#[test]
fn escaped_length_matches_serde_json() {
    for text in [
        "plain",
        "a\"b\\c",
        "line\nbreak\ttab\r",
        "\u{01}\u{1f}",
        "çşğ ✓",
        "",
    ] {
        let written = serde_json::to_string(text).unwrap();
        assert_eq!(escaped_len(text), written.len() - 2, "{text:?}");
    }
}

#[test]
fn bisection_finds_the_largest_even_size_that_fits() {
    assert_eq!(chunk_size(|size| size <= 1_000), Some(1_000));
    assert_eq!(chunk_size(|size| size <= 1_001), Some(1_000));
    assert_eq!(chunk_size(|size| size <= 2), Some(2));
    assert_eq!(chunk_size(|_| false), None);
    assert_eq!(chunk_size(|_| true), Some(1_048_576));
}

#[test]
fn documents_split_at_line_breaks_then_whitespace_then_characters() {
    let lines = "first line\nsecond line\nthird line\n";
    let chunks = split(lines, 24);
    assert_eq!(&lines[chunks[0].clone()], "first line\n");
    assert_eq!(rejoined(lines, 24), lines);

    let words = "alpha beta gamma delta";
    assert_eq!(&words[split(words, 12)[0].clone()], "alpha beta ");
    assert_eq!(rejoined(words, 12), words);

    let long = "x".repeat(25);
    assert_eq!(split(&long, 10).len(), 3);
    assert_eq!(rejoined(&long, 10), long);

    let fits = "a\nb";
    assert_eq!(split(fits, 10), vec![0..3]);

    let quoted = "{\"k\": \"v\\\\\"}\n".repeat(40);
    assert_eq!(rejoined(&quoted, 64), quoted);

    let turkish = "Şükrü İşçi çalıştı.\n".repeat(30);
    assert_eq!(rejoined(&turkish, 50), turkish);
}
