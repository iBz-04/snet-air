use snet_rules::words::for_each_word;

fn collect_words(text: &str) -> Vec<(String, usize)> {
    let mut words = Vec::new();
    for_each_word(text.as_bytes(), |word, line| {
        words.push((String::from_utf8_lossy(word).into_owned(), line));
    });
    words
}

#[test]
fn splits_on_punctuation_and_lowercases() {
    assert_eq!(
        collect_words("Hello, cloud_client!"),
        vec![
            ("hello".to_string(), 1),
            ("cloud".to_string(), 1),
            ("client".to_string(), 1)
        ]
    );
}

#[test]
fn yields_camel_case_parts_and_the_whole_run() {
    assert_eq!(
        collect_words("CloudClient v2Plan"),
        vec![
            ("cloud".to_string(), 1),
            ("client".to_string(), 1),
            ("cloudclient".to_string(), 1),
            ("v2".to_string(), 1),
            ("plan".to_string(), 1),
            ("v2plan".to_string(), 1)
        ]
    );
}

#[test]
fn keeps_runs_of_capitals_together() {
    assert_eq!(
        collect_words("HTTPServer"),
        vec![("httpserver".to_string(), 1)]
    );
}

#[test]
fn reports_the_line_each_word_starts_on() {
    assert_eq!(
        collect_words("one\n\ntwo three\nfour"),
        vec![
            ("one".to_string(), 1),
            ("two".to_string(), 3),
            ("three".to_string(), 3),
            ("four".to_string(), 4)
        ]
    );
}

#[test]
fn non_ascii_bytes_separate_words() {
    assert_eq!(
        collect_words("caf\u{e9}bar"),
        vec![("caf".to_string(), 1), ("bar".to_string(), 1)]
    );
}
