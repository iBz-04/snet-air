use super::HeadLimit;

#[test]
fn a_head_at_the_limit_passes_and_one_byte_more_fails() {
    let head = |length: usize| {
        let mut bytes = b"HTTP/1.1 200 OK\r\nx: ".to_vec();
        bytes.resize(length - 4, b'v');
        bytes.extend_from_slice(b"\r\n\r\n");
        bytes
    };
    let mut limit = HeadLimit::new(100);
    assert!(limit.observe(&head(100)).is_ok());
    let mut limit = HeadLimit::new(100);
    assert!(limit.observe(&head(101)).is_err());
}

#[test]
fn the_end_of_the_head_is_found_across_reads_and_the_body_is_not_counted() {
    let mut limit = HeadLimit::new(40);
    assert!(limit.observe(b"HTTP/1.1 200 OK\r\na: b\r").is_ok());
    assert!(limit.observe(b"\n\r").is_ok());
    assert!(
        limit
            .observe(b"\nbody that is much longer than the head limit allows")
            .is_ok()
    );
    assert!(limit.observe(&[b'x'; 1000]).is_ok());
}

#[test]
fn a_head_split_into_many_reads_is_still_counted() {
    let mut limit = HeadLimit::new(50);
    let mut result = Ok(());
    for _ in 0..60 {
        result = limit.observe(b"v");
        if result.is_err() {
            break;
        }
    }
    assert!(result.is_err());
}
