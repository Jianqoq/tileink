use super::{COARSE_CONFIG, Interface, Kind, buffer, interface, uniform};
use std::{collections::BTreeMap, io};

pub(super) fn get(family: &str, constants: &BTreeMap<String, u32>) -> io::Result<Interface> {
    Ok(match family {
        "coarse-emit-allocation" => interface(
            [constants["COARSE_WORKGROUP_SIZE"], 1, 1],
            &[
                ("config", uniform(0, COARSE_CONFIG, false)),
                ("coarse_work", buffer(7, Kind::Write)),
                ("chunk_records", buffer(8, Kind::Write)),
            ],
            &[
                ("coarse_emit_chunk_counts", &["config", "coarse_work"]),
                (
                    "coarse_emit_prefix_chunks",
                    &["config", "coarse_work", "chunk_records"],
                ),
                (
                    "coarse_emit_apply_chunk_offsets",
                    &["config", "coarse_work", "chunk_records"],
                ),
                ("coarse_emit_fill_refs", &["config", "coarse_work"]),
            ],
        ),
        "coarse-emit-offsets" => interface(
            [1, 1, 1],
            &[
                ("config", uniform(0, COARSE_CONFIG, false)),
                ("coarse_work", buffer(7, Kind::Write)),
                ("chunk_records", buffer(8, Kind::Write)),
            ],
            &[("coarse_emit_chunk_offsets", &["config", "chunk_records"])],
        ),
        "coarse-prefix" => interface(
            [constants["COARSE_WORKGROUP_SIZE"], 1, 1],
            &[
                ("config", uniform(0, COARSE_CONFIG, false)),
                ("coarse_work", buffer(7, Kind::Write)),
                ("chunk_records", buffer(8, Kind::Write)),
            ],
            &[
                (
                    "coarse_prefix_chunks",
                    &["config", "coarse_work", "chunk_records"],
                ),
                ("coarse_chunk_offsets", &["config", "chunk_records"]),
                (
                    "coarse_apply_chunk_offsets",
                    &["config", "coarse_work", "chunk_records"],
                ),
            ],
        ),
        _ => return Err(io::Error::other(format!("unknown shader family: {family}"))),
    })
}
