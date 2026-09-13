//! Versioned minimum-probe contract. A partial inventory must not produce a
//! successful native build; expanding this contract requires a new schema.
use std::{collections::BTreeSet, io};

pub fn validate(abi: &serde_json::Value) -> io::Result<()> {
    let expected = serde_json::json!({
        "schema":1,"workgroup":[64,1,1],"parameter_size":32,"parameter_alignment":16,
        "parameter_offsets":{"count":0,"source_offset":4,"destination_offset":8,"stride":12,"value":16},
        "bindings":{"destination":0,"source":1,"params":2},"descriptor_set":0,
        "buffer_offsets_alignment":4,"layout_output_stride_minimum":16,"integer_arithmetic":"uint32 wrap"
    });
    for (field, value) in expected.as_object().unwrap() {
        if &abi[field] != value {
            return Err(io::Error::other(format!(
                "native probe ABI mismatch: {field}"
            )));
        }
    }
    let programs = abi["programs"]
        .as_array()
        .ok_or_else(|| io::Error::other("missing native probe programs"))?;
    let names: BTreeSet<_> = programs.iter().filter_map(|v| v.as_str()).collect();
    if programs.len() != 4
        || names != BTreeSet::from(["clear_words", "copy_words", "layout_words", "sample_words"])
    {
        return Err(io::Error::other(
            "native probe programs must contain clear_words, copy_words, layout_words and sample_words exactly once",
        ));
    }
    Ok(())
}
