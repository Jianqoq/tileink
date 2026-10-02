use std::{path::PathBuf, time::SystemTime};

use png::{BitDepth, ColorType, Compression};

use super::*;

fn encode(
    size: (u32, u32),
    color: ColorType,
    depth: BitDepth,
    data: &[u8],
    configure: impl FnOnce(&mut png::Encoder<'_, &mut Vec<u8>>),
) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, size.0, size.1);
    encoder.set_color(color);
    encoder.set_depth(depth);
    configure(&mut encoder);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(data).unwrap();
    writer.finish().unwrap();
    bytes
}

fn rgba(data: &[u8]) -> Vec<u8> {
    encode((2, 1), ColorType::Rgba, BitDepth::Eight, data, |_| {})
}

#[test]
fn compression_filters_and_metadata_do_not_change_pixels() {
    let data = [12, 34, 56, 255].repeat(128);
    let encoded = |compression, text: &str| {
        encode(
            (16, 8),
            ColorType::Rgba,
            BitDepth::Eight,
            &data,
            |encoder| {
                encoder.set_compression(compression);
                encoder
                    .add_text_chunk("Comment".into(), text.into())
                    .unwrap();
            },
        )
    };
    let before = encoded(Compression::NoCompression, "old encoder");
    let after = encoded(Compression::High, "new encoder");
    assert_ne!(before, after);
    assert_eq!(pixels::compare(&before, &after).unwrap(), None);
}

#[test]
fn color_types_and_sample_depths_compare_as_lossless_rgba() {
    for (color, depth, data, expected) in [
        (
            ColorType::Grayscale,
            BitDepth::One,
            vec![0x40],
            vec![0, 0, 0, 255, 255, 255, 255, 255],
        ),
        (
            ColorType::Grayscale,
            BitDepth::Two,
            vec![0x60],
            vec![85, 85, 85, 255, 170, 170, 170, 255],
        ),
        (
            ColorType::Grayscale,
            BitDepth::Four,
            vec![0x5a],
            vec![85, 85, 85, 255, 170, 170, 170, 255],
        ),
        (
            ColorType::Grayscale,
            BitDepth::Eight,
            vec![85, 170],
            vec![85, 85, 85, 255, 170, 170, 170, 255],
        ),
        (
            ColorType::GrayscaleAlpha,
            BitDepth::Eight,
            vec![85, 16, 170, 32],
            vec![85, 85, 85, 16, 170, 170, 170, 32],
        ),
        (
            ColorType::Rgb,
            BitDepth::Eight,
            vec![1, 2, 3, 4, 5, 6],
            vec![1, 2, 3, 255, 4, 5, 6, 255],
        ),
        (
            ColorType::Indexed,
            BitDepth::One,
            vec![0x40],
            vec![1, 2, 3, 0, 4, 5, 6, 128],
        ),
        (
            ColorType::Rgba,
            BitDepth::Sixteen,
            vec![1, 1, 2, 2, 3, 3, 255, 255, 4, 4, 5, 5, 6, 6, 0, 0],
            vec![1, 2, 3, 255, 4, 5, 6, 0],
        ),
    ] {
        let before = encode((2, 1), color, depth, &data, |encoder| {
            if color == ColorType::Indexed {
                encoder.set_palette(vec![1, 2, 3, 4, 5, 6]);
                encoder.set_trns(vec![0, 128]);
            }
        });
        assert_eq!(
            pixels::compare(&before, &rgba(&expected)).unwrap(),
            None,
            "{color:?}/{depth:?}"
        );
    }
}

#[test]
fn detects_one_step_in_every_channel_even_under_zero_alpha() {
    let before = [10, 20, 30, 255, 40, 50, 60, 0];
    for channel in 0..4 {
        let mut after = before;
        after[4 + channel] += 1;
        assert_eq!(
            pixels::compare(&rgba(&before), &rgba(&after)).unwrap(),
            Some(pixels::Difference::Pixels {
                count: 1,
                first: (1, 0),
                before: [40, 50, 60, 0].map(|value| value * 257),
                after: after[4..]
                    .try_into()
                    .map(|values: [u8; 4]| values.map(|v| u16::from(v) * 257))
                    .unwrap(),
            })
        );
    }
    let encode16 = |data: &[u8]| encode((1, 1), ColorType::Rgba, BitDepth::Sixteen, data, |_| {});
    let before = encode16(&[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0, 0]);
    let after = encode16(&[0x12, 0x35, 0x56, 0x78, 0x9a, 0xbc, 0, 0]);
    assert!(matches!(
        pixels::compare(&before, &after).unwrap(),
        Some(pixels::Difference::Pixels { count: 1, .. })
    ));
}

#[test]
fn reports_dimensions_and_counts_all_changed_pixels() {
    let before = rgba(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let resized = encode(
        (1, 2),
        ColorType::Rgba,
        BitDepth::Eight,
        &[1, 2, 3, 4, 5, 6, 7, 8],
        |_| {},
    );
    assert_eq!(
        pixels::compare(&before, &resized).unwrap(),
        Some(pixels::Difference::Dimensions {
            before: (2, 1),
            after: (1, 2)
        })
    );
    assert!(matches!(
        pixels::compare(&before, &rgba(&[0; 8])).unwrap(),
        Some(pixels::Difference::Pixels {
            count: 2,
            first: (0, 0),
            ..
        })
    ));
}

#[test]
fn corrupt_truncated_and_animated_images_cannot_pass() {
    let valid = rgba(&[0; 8]);
    let animated = encode(
        (2, 1),
        ColorType::Rgba,
        BitDepth::Eight,
        &[0; 8],
        |encoder| {
            encoder.set_animated(1, 0).unwrap();
        },
    );
    for invalid in [
        &b"not a PNG"[..],
        &valid[..valid.len() / 2],
        &valid[..valid.len() - 12],
        &animated,
    ] {
        assert!(pixels::compare(&valid, invalid).is_err());
        assert!(pixels::compare(invalid, invalid).is_err());
    }
}

struct Repository(PathBuf);

impl Repository {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("tileink-png-pixels-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let repo = Self(root);
        repo.git(&["init", "--quiet"]);
        repo
    }

    fn git(&self, args: &[&str]) {
        git::command(&self.0, args).unwrap();
    }

    fn commit(&self) {
        self.git(&[
            "-c",
            "user.name=PNG test",
            "-c",
            "user.email=png@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=nonexistent-hooks",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "PNG baseline",
        ]);
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn git_baseline_checks_all_pngs_without_rewriting_them_or_trusting_the_index() {
    let repo = Repository::new();
    repo.commit();
    assert!(check(&repo.0, "HEAD", &mut Vec::new()).is_err());
    let original = rgba(&[0; 8]);
    fs::create_dir(repo.0.join("space 路径")).unwrap();
    for path in [
        "unchanged.png",
        "encoding.png",
        "space 路径/sample.PNG",
        "deleted.png",
        "corrupt.png",
        "pixels.png",
    ] {
        fs::write(repo.0.join(path), &original).unwrap();
    }
    repo.git(&["add", "."]);
    repo.commit();
    assert_eq!(
        check(&repo.0, "HEAD", &mut Vec::new()).unwrap(),
        Summary {
            byte_equal: 6,
            ..Summary::default()
        }
    );
    assert!(check(&repo.0, "no-such-revision", &mut Vec::new()).is_err());

    let changed = rgba(&[1; 8]);
    fs::write(repo.0.join("unchanged.png"), &changed).unwrap();
    repo.git(&["add", "unchanged.png"]);
    fs::write(repo.0.join("unchanged.png"), &original).unwrap();
    let reencoded = encode(
        (2, 1),
        ColorType::Rgba,
        BitDepth::Eight,
        &[0; 8],
        |encoder| encoder.set_compression(Compression::NoCompression),
    );
    assert_ne!(original, reencoded);
    fs::write(repo.0.join("encoding.png"), &reencoded).unwrap();
    fs::remove_file(repo.0.join("deleted.png")).unwrap();
    fs::write(repo.0.join("corrupt.png"), b"broken").unwrap();
    fs::write(repo.0.join("pixels.png"), &changed).unwrap();
    fs::write(repo.0.join("new image.png"), &original).unwrap();
    for staged in [false, true] {
        if staged {
            repo.git(&["add", "."]);
        }
        let mut report = Vec::new();
        assert_eq!(
            check(&repo.0, "HEAD", &mut report).unwrap(),
            Summary {
                byte_equal: 2,
                pixel_equal: 1,
                failed: 4
            }
        );
        let report = String::from_utf8(report).unwrap();
        for status in [
            "SAME_BYTES",
            "SAME_PIXELS",
            "NO_BASELINE",
            "UNREADABLE",
            "DECODE_ERROR",
            "DIFF",
            "Result: FAIL",
        ] {
            assert!(report.contains(status), "missing {status}: {report}");
        }
        assert_eq!(fs::read(repo.0.join("encoding.png")).unwrap(), reencoded);
        assert_eq!(fs::read(repo.0.join("pixels.png")).unwrap(), changed);
    }
    let old = check(&repo.0, "HEAD~1", &mut Vec::new()).unwrap();
    assert_eq!(old.byte_equal + old.pixel_equal, 0);
    assert!(old.failed > 0);
}
