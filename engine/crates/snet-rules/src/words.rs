/// Calls `visit` with every word of `bytes`, lowercased, and the 1-based line
/// it is on. A word is a run of ASCII letters and digits, and also each part
/// of that run split where a lowercase letter or digit is followed by an
/// uppercase letter, so `CloudClient` yields `cloud`, `client` and
/// `cloudclient`. Every other byte, including non-ASCII ones, separates words.
pub fn for_each_word(bytes: &[u8], mut visit: impl FnMut(&[u8], usize)) {
    let mut run = WordRun::default();
    let mut line = 1;

    for &byte in bytes {
        if byte.is_ascii_alphanumeric() {
            run.push(byte, line, &mut visit);
        } else {
            run.finish(&mut visit);
            if byte == b'\n' {
                line += 1;
            }
        }
    }
    run.finish(&mut visit);
}

#[derive(Default)]
struct WordRun {
    whole_run: Vec<u8>,
    current_part: Vec<u8>,
    has_several_parts: bool,
    previous_byte: Option<u8>,
    line: usize,
}

impl WordRun {
    fn push(&mut self, byte: u8, line: usize, visit: &mut impl FnMut(&[u8], usize)) {
        let starts_new_part = byte.is_ascii_uppercase()
            && self
                .previous_byte
                .is_some_and(|previous| previous.is_ascii_lowercase() || previous.is_ascii_digit());
        if starts_new_part {
            visit(&self.current_part, self.line);
            self.current_part.clear();
            self.has_several_parts = true;
        }
        if self.whole_run.is_empty() {
            self.line = line;
        }
        let lowercase = byte.to_ascii_lowercase();
        self.whole_run.push(lowercase);
        self.current_part.push(lowercase);
        self.previous_byte = Some(byte);
    }

    fn finish(&mut self, visit: &mut impl FnMut(&[u8], usize)) {
        if self.whole_run.is_empty() {
            return;
        }
        visit(&self.current_part, self.line);
        if self.has_several_parts {
            visit(&self.whole_run, self.line);
        }
        self.whole_run.clear();
        self.current_part.clear();
        self.has_several_parts = false;
        self.previous_byte = None;
    }
}
