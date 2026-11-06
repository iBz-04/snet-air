use super::*;

fn words(text: &str) -> Vec<String> {
    text.split(' ').map(str::to_string).collect()
}

fn ask_of(text: &str) -> AskArguments {
    match parse(&words(text)).unwrap() {
        Command::Ask(arguments) => *arguments,
        other => panic!("{other:?}"),
    }
}

fn refused(text: &str) -> String {
    parse(&words(text)).unwrap_err().message
}

#[test]
fn a_question_and_files_are_all_ask_needs() {
    let parsed = ask_of("ask Who? a.txt b.txt");
    assert_eq!(parsed.question, "Who?");
    assert_eq!(parsed.files, ["a.txt", "b.txt"]);
    assert!(!parsed.fanout);
    assert_eq!(parsed.cloud, None);
}

#[test]
fn every_option_is_read_before_or_between_positionals() {
    let parsed = ask_of(
        "ask --fanout --cloud openai:gpt-x Who? --local ollama:llama3 a.txt --context 16384 --effort high --rounds 3 --plan-rounds 1 --policy p.toml --cloud-url https://h/v1",
    );
    assert!(parsed.fanout);
    assert_eq!(parsed.cloud.as_deref(), Some("openai:gpt-x"));
    assert_eq!(parsed.local.as_deref(), Some("ollama:llama3"));
    assert_eq!(parsed.context, Some(16_384));
    assert_eq!(parsed.effort.as_deref(), Some("high"));
    assert_eq!(parsed.rounds, Some(3));
    assert_eq!(parsed.plan_rounds, Some(1));
    assert_eq!(parsed.policy, Some(PathBuf::from("p.toml")));
    assert_eq!(parsed.cloud_url.as_deref(), Some("https://h/v1"));
    assert_eq!(parsed.question, "Who?");
    assert_eq!(parsed.files, ["a.txt"]);
}

#[test]
fn a_double_dash_ends_the_options() {
    let parsed = ask_of("ask -- --odd-question --file.txt");
    assert_eq!(parsed.question, "--odd-question");
    assert_eq!(parsed.files, ["--file.txt"]);
}

#[test]
fn help_and_version_are_commands() {
    assert_eq!(parse(&words("--help")).unwrap(), Command::Help);
    assert_eq!(parse(&words("help")).unwrap(), Command::Help);
    assert_eq!(parse(&words("--version")).unwrap(), Command::Version);
    assert_eq!(parse(&[]).unwrap(), Command::Interactive);
    assert_eq!(parse(&words("pull")).unwrap(), Command::Pull(None));
    assert_eq!(
        parse(&words("pull qwen3:8b")).unwrap(),
        Command::Pull(Some("qwen3:8b".to_string()))
    );
}

#[test]
fn mistakes_are_named() {
    assert!(refused("nonsense").starts_with("usage: snet"));
    assert!(refused("ask").contains("needs a question"));
    assert!(parse(&words("ask Who?")).is_ok());
    assert!(refused("ask Who? a.txt a.txt").contains("named twice"));
    assert!(refused("ask --rounds").contains("needs a value"));
    assert!(refused("ask --rounds two Who? a.txt").contains("whole number"));
    assert!(refused("ask --fanout --fanout Who? a.txt").contains("given twice"));
    assert!(refused("ask --cloud a:b --cloud c:d Who? a.txt").contains("given twice"));
    assert!(refused("ask --colour red Who? a.txt").contains("unknown option --colour"));
}

#[test]
fn arguments_that_are_not_utf8_are_refused() {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let error = from_os(vec![
            OsString::from("snet"),
            OsString::from("ask"),
            OsString::from_vec(vec![0xff]),
        ])
        .unwrap_err();
        assert_eq!(error.message, "argument 2 is not UTF-8");
    }
}
