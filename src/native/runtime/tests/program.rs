use super::*;

fn command() -> Dispatch {
    Dispatch {
        entry: "copy_words",
        params: Params {
            count: 1,
            source_offset: 0,
            destination_offset: 0,
            stride: 4,
            value: [0; 4],
        },
        source: vec![0; 4],
        destination: vec![0; 4],
    }
}

#[test]
fn bounds_and_alignment_are_checked_before_native_recording() {
    assert!(command().validate().is_ok());
    let mut c = command();
    c.params.count = 0;
    assert!(c.validate().is_ok());
    for c in [
        Dispatch {
            entry: "missing",
            ..command()
        },
        Dispatch {
            source: vec![],
            ..command()
        },
        Dispatch {
            destination: vec![0; 3],
            ..command()
        },
    ] {
        assert!(c.validate().is_err());
    }
    let mut c = command();
    c.params.count = 2;
    assert!(c.validate().is_err());
    let mut c = command();
    c.params.source_offset = 1;
    assert!(c.validate().is_err());
    let mut c = command();
    c.params.destination_offset = u32::MAX - 3;
    assert!(c.validate().is_err());
    let mut c = command();
    c.params.count = u32::MAX;
    assert!(c.validate().is_err());
    let mut c = command();
    c.entry = "layout_words";
    c.params.stride = 12;
    assert!(c.validate().is_err());
    assert!(validate_batch(&[]).is_err());
    assert!(validate_batch(&vec![command(); 4097]).is_err());
}

#[test]
fn sampling_rejects_nonfinite_overflow_and_missing_texels() {
    let mut c = command();
    c.entry = "sample_words";
    c.params.value = [0, 0, 1, 0];
    assert!(c.validate().is_ok());
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
        let mut bad = c.clone();
        bad.params.value[0] = value.to_bits();
        assert!(bad.validate().is_err());
        let mut bad = c.clone();
        bad.params.value[1] = value.to_bits();
        assert!(bad.validate().is_err());
    }
    for width in [0, 2, u32::MAX] {
        let mut bad = c.clone();
        bad.params.value[2] = width;
        assert!(bad.validate().is_err());
    }
}

#[test]
fn fixed_coordinates_reject_signed_minimum_and_overflowing_products() {
    let mut c = command();
    c.entry = "sample_words";
    c.params.value = [(-32768.0f32).to_bits(), 0, 1, 0];
    assert!(c.validate().is_err()); // Never negate i32::MIN in HLSL/MSL.
    for origin in [f32::from_bits(0x46ffffff), -f32::from_bits(0x46ffffff)] {
        c.params.value[0] = origin.to_bits();
        assert!(c.validate().is_ok());
    }
    c.params.count = 2;
    c.destination.resize(8, 0);
    c.params.value = [0, 32768.0f32.to_bits(), 1, 0];
    assert!(c.validate().is_err());
    c.params.count = 3;
    c.destination.resize(12, 0);
    c.params.value = [0, 16384.0f32.to_bits(), 1, 0];
    assert!(c.validate().is_err());
    c.params.value = [16384.0f32.to_bits(), 8192.0f32.to_bits(), 1, 0];
    assert!(c.validate().is_err());
}
