use atarustia::{
    file_id::{FileKind, FileRef},
    forms::{Forms, Screen},
    media::{Media, Source},
    modules::Package,
};
use grammers_client::{media::Uploaded, tl};

#[test]
fn cached_photo_and_document_become_typed_inline_results() {
    for kind in [
        FileKind::Photo,
        FileKind::Video,
        FileKind::Audio,
        FileKind::Voice,
        FileKind::Sticker,
    ] {
        let file = FileRef {
            kind,
            dc: 2,
            id: 100,
            access_hash: 200,
            reference: vec![3],
            thumb: if kind == FileKind::Photo {
                "x".into()
            } else {
                String::new()
            },
        };
        let media = Media {
            kind,
            source: Source::FileId {
                file_id: file.encode().unwrap(),
            },
            ..Default::default()
        };
        let result = atarustia::media::inline_result(
            "result",
            "cached",
            grammers_client::message::InputMessage::new().text("caption"),
            Some(&media),
            None,
        )
        .unwrap();
        assert!(match result {
            tl::enums::InputBotInlineResult::Photo(p) =>
                kind == FileKind::Photo
                    && matches!(
                        p.send_message,
                        tl::enums::InputBotInlineMessage::MediaAuto(_)
                    ),
            tl::enums::InputBotInlineResult::Document(p) =>
                kind != FileKind::Photo
                    && matches!(
                        p.send_message,
                        tl::enums::InputBotInlineMessage::MediaAuto(_)
                    ),
            _ => false,
        });
    }
}
#[test]
fn uploaded_document_preserves_semantics() {
    let uploaded = || Uploaded {
        raw: tl::types::InputFile {
            id: 1,
            parts: 1,
            name: "clip.mp4".into(),
            md5_checksum: String::new(),
        }
        .into(),
    };
    for kind in [
        FileKind::Video,
        FileKind::Animation,
        FileKind::VideoNote,
        FileKind::Document,
    ] {
        let media = Media {
            kind,
            source: Source::Bytes {
                name: "clip.mp4".into(),
                data: vec![1],
            },
            duration: 1.5,
            width: 320,
            height: 240,
            ..Default::default()
        };
        let input = media.input(Some(uploaded())).unwrap();
        let tl::enums::InputMedia::UploadedDocument(input) = input else {
            panic!("expected upload")
        };
        assert_eq!(input.force_file, kind == FileKind::Document);
        assert_eq!(input.nosound_video, kind == FileKind::Animation);
        assert!(input.attributes.iter().any(
            |a| matches!(a,tl::enums::DocumentAttribute::Filename(n) if n.file_name=="clip.mp4")
        ));
    }
}
#[test]
fn module_gallery_round_trip_without_network() {
    let p: Package = serde_json::from_str(include_str!("../examples/gallery.module.json")).unwrap();
    p.validate().unwrap();
    let effect = p
        .run_command(
            "gallery",
            "",
            p.manifest.config.clone(),
            serde_json::Value::Null,
        )
        .unwrap();
    let screen = effect.screen.unwrap();
    screen.restrict_module("gallery").unwrap();
    let mut forms = Forms::default();
    let (id, _) = forms.create(screen, 1, vec![]).unwrap();
    let first = forms.inline_result(&id).unwrap();
    assert!(matches!(first,tl::enums::InputBotInlineResult::Result(r) if r.r#type=="photo"));
    forms.render(&id, 1).unwrap();
    assert!(
        matches!(forms.media(&id).unwrap().source,Source::Url{url} if url.ends_with("second.jpg"))
    );
}
#[test]
fn malformed_media_cannot_create_a_unit() {
    let mut forms = Forms::default();
    let screen = Screen {
        media: Some(Media::from_url(
            FileKind::Photo,
            "http://example.com/unsafe.jpg",
        )),
        ..Default::default()
    };
    assert!(forms.create(screen, 1, vec![]).is_err());
    let screen = Screen {
        media: Some(Media::from_url(
            FileKind::Photo,
            "https://example.com/a.jpg",
        )),
        text: "x".repeat(1025),
        ..Default::default()
    };
    assert!(screen.validate().is_err());
}
#[test]
fn error_mapping_keeps_method_and_retry_after() {
    let rpc = grammers_client::sender::RpcError {
        code: 420,
        name: "FLOOD_WAIT".into(),
        value: Some(17),
        caused_by: None,
    };
    let error = grammers_client::InvocationError::Rpc(rpc);
    let mapped = atarustia::botapi::ApiError::from_rpc("send_video", &error);
    assert_eq!(mapped.kind, atarustia::botapi::ErrorKind::RetryAfter);
    assert_eq!(mapped.retry_after, Some(17));
    assert_eq!(mapped.method, "send_video");
}
#[test]
fn file_metadata_from_native_media_and_bounded_cache() {
    let raw = tl::types::Document {
        id: 123,
        access_hash: 456,
        file_reference: vec![0, 1, 2],
        date: 0,
        mime_type: "audio/ogg".into(),
        size: 789,
        thumbs: None,
        video_thumbs: None,
        dc_id: 2,
        attributes: vec![
            tl::types::DocumentAttributeFilename {
                file_name: "voice.ogg".into(),
            }
            .into(),
            tl::types::DocumentAttributeAudio {
                voice: true,
                duration: 5,
                title: None,
                performer: None,
                waveform: None,
            }
            .into(),
        ],
    };
    let native = grammers_client::media::Media::Document(
        grammers_client::media::Document::from_raw_media(tl::types::MessageMediaDocument {
            nopremium: false,
            spoiler: false,
            video: false,
            round: false,
            voice: true,
            document: Some(raw.into()),
            alt_documents: None,
            video_cover: None,
            video_timestamp: None,
            ttl_seconds: None,
        }),
    );
    let file = atarustia::botapi::File::from_media(&native).unwrap();
    assert_eq!(file.file_size, Some(789));
    assert_eq!(file.file_name.as_deref(), Some("voice.ogg"));
    assert_eq!(
        FileRef::decode(&file.file_id).unwrap().kind,
        FileKind::Voice
    );
    let dir = tempfile::tempdir().unwrap();
    let db = atarustia::database::Database::open(dir.path(), 1).unwrap();
    for n in 0..260 {
        atarustia::assets::remember(&db, &format!("bot:{n:04}"), &native).unwrap();
    }
    assert_eq!(db.get("RustMedia", "cache").as_object().unwrap().len(), 256);
    atarustia::assets::forget(&db, "bot:0259").unwrap();
    assert!(atarustia::assets::cached(&db, "bot:0259").is_none());
}
#[test]
fn all_examples_with_media_validate() {
    let package: Package =
        serde_json::from_str(include_str!("../examples/gallery.module.json")).unwrap();
    package.validate().unwrap();
    let effect = package
        .run_command(
            "gallery",
            "",
            package.manifest.config.clone(),
            serde_json::Value::Null,
        )
        .unwrap();
    assert_eq!(effect.screen.unwrap().gallery.len(), 2);
}
