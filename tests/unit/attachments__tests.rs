use super::*;

fn staged_with(n: usize) -> Staged {
    let mut s = Staged::default();
    for i in 0..n {
        s.add(
            format!("img-{i}.png"),
            "clipboard".into(),
            "image/png".into(),
            vec![i as u8],
        )
        .unwrap();
    }
    s
}

#[test]
fn add_mints_stable_unique_tokens_and_caps() {
    let mut s = staged_with(MAX_STAGED);
    assert!(s
        .add("x".into(), "p".into(), "image/png".into(), vec![])
        .is_err());
    let tokens: Vec<&str> = s.iter().map(|a| a.token.as_str()).collect();
    assert_eq!(tokens[0], "[image 1]");
    assert_eq!(tokens[7], "[image 8]");
    // Tokens never reindex after a removal.
    s.remove(0);
    assert_eq!(s.get(0).unwrap().token, "[image 2]");
}

#[test]
fn reconcile_drops_attachments_whose_token_left_the_text() {
    let mut s = staged_with(3);
    let dropped = s.reconcile("keep [image 1] and [image 3] only");
    assert_eq!(dropped, 1);
    let tokens: Vec<&str> = s.iter().map(|a| a.token.as_str()).collect();
    assert_eq!(tokens, ["[image 1]", "[image 3]"]);
    // A broken token counts as gone.
    let dropped = s.reconcile("[image 1 …oops [image 3]");
    assert_eq!(dropped, 1);
    assert_eq!(s.len(), 1);
    assert_eq!(s.reconcile(""), 1);
    assert!(s.is_empty());
}

#[test]
fn png_sniffing_reads_the_magic() {
    let mut s = Staged::default();
    s.add(
        "a.png".into(),
        "p".into(),
        "image/png".into(),
        b"\x89PNG\r\n\x1a\nrest".to_vec(),
    )
    .unwrap();
    s.add(
        "b.jpg".into(),
        "p".into(),
        "image/jpeg".into(),
        vec![0xff, 0xd8],
    )
    .unwrap();
    assert!(s.get(0).unwrap().is_png());
    assert!(!s.get(1).unwrap().is_png());
}
