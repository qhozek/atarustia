use atarustia::rich_media;
use grammers_client::tl::{self, Deserializable, Serializable};

#[test]
fn rich_inline_photos_are_bound_to_uploaded_references_not_external_urls() {
    let source = r#"<img tg-spoiler src='https://example.com/a.png?x=1&amp;y=2'/><h2>Help</h2><details><summary>Modules</summary>Body</details>"#;
    let photos = rich_media::photos(source).unwrap();
    assert_eq!(photos.len(), 1);
    assert_eq!(photos[0].url, "https://example.com/a.png?x=1&y=2");
    let photo: tl::enums::InputPhoto = tl::types::InputPhoto {
        id: 123,
        access_hash: 456,
        file_reference: vec![1, 2, 3],
    }
    .into();
    let bound = rich_media::bind(source, &[(photos[0].clone(), photo.clone())]);
    let bytes = bound.to_bytes();
    let mut cursor = tl::deserialize::Cursor::from_slice(&bytes);
    let tl::enums::InputRichMessage::Html(bound) =
        tl::enums::InputRichMessage::deserialize(&mut cursor).unwrap()
    else {
        panic!()
    };
    assert!(!bound.html.contains("https://example.com"));
    assert!(bound.html.contains("tg-spoiler"));
    assert!(bound.html.contains("<details>"));
    let files = bound.files.unwrap();
    let tl::enums::InputRichFile::Photo(file) = &files[0] else {
        panic!()
    };
    assert_eq!(file.photo, photo);
    assert!(
        file.id.len() <= 64
            && file
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    );
    assert!(bound.html.contains(&format!("tg://photo?id={}", file.id)));
}

#[test]
fn rich_photo_scan_keeps_code_comments_emoji_and_existing_references_untouched() {
    let source = r#"<!-- <img src="https://example.com/ignored.png"> --><pre><img src="https://example.com/code.png"></pre><code><img src="https://example.com/code.png"></code><img src="tg://emoji?id=123" alt="★"/><img src="tg://photo?id=existing"/><IMG SRC="https://example.com/a.png" title="a > b"/>"#;
    let photos = rich_media::photos(source).unwrap();
    assert_eq!(photos.len(), 1);
    let bound = rich_media::bind(
        source,
        &[(
            photos[0].clone(),
            tl::types::InputPhoto {
                id: 123,
                access_hash: 456,
                file_reference: vec![],
            }
            .into(),
        )],
    );
    let tl::enums::InputRichMessage::Html(bound) = bound else {
        panic!()
    };
    assert!(bound.html.contains("https://example.com/ignored.png"));
    assert!(bound.html.contains("https://example.com/code.png"));
    assert!(bound.html.contains("tg://emoji?id=123"));
    assert!(bound.html.contains("tg://photo?id=existing"));
    assert!(bound.html.contains("title=\"a &gt; b\""));
}

#[test]
fn rich_photo_scan_enforces_existing_https_policy_and_media_budget() {
    assert!(rich_media::photos(r#"<img src="https://user:pass@example.com/a.png"/>"#).is_err());
    assert!(rich_media::photos(r#"<img src="http://example.com/a.png"/>"#).is_err());
    assert!(rich_media::photos(&r#"<img src="https://example.com/a.png"/>"#.repeat(51)).is_err());
}

#[test]
fn repeated_photo_tags_receive_distinct_valid_bindings_without_rewriting_comments() {
    let tag = r#"<img src="https://example.com/a.png"/>"#;
    let source = format!("<!-- {tag} -->{tag}{tag}");
    let references = rich_media::photos(&source)
        .unwrap()
        .into_iter()
        .map(|photo| {
            (
                photo,
                tl::types::InputPhoto {
                    id: 123,
                    access_hash: 456,
                    file_reference: vec![1],
                }
                .into(),
            )
        })
        .collect::<Vec<_>>();
    let tl::enums::InputRichMessage::Html(bound) = rich_media::bind(&source, &references) else {
        panic!()
    };
    assert!(bound.html.starts_with(&format!("<!-- {tag} -->")));
    let files = bound.files.unwrap();
    assert_eq!(files.len(), 2);
    let tl::enums::InputRichFile::Photo(first) = &files[0] else {
        panic!()
    };
    let tl::enums::InputRichFile::Photo(second) = &files[1] else {
        panic!()
    };
    assert_ne!(first.id, second.id);
    assert!(bound.html.contains(&format!("tg://photo?id={}", first.id)));
    assert!(bound.html.contains(&format!("tg://photo?id={}", second.id)));
}
