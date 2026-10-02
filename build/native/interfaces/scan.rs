use super::{DISPATCH_GRID, Interface, Kind, SCAN_CONFIG, buffer, interface, uniform};
use std::{collections::BTreeMap, io};

pub(super) fn get(family: &str, constants: &BTreeMap<String, u32>) -> io::Result<Interface> {
    Ok(match family {
        "scan-apply-chunk-offsets" => interface(
            [constants["SCAN_CHUNK_SIZE"], 1, 1],
            &[
                ("config", uniform(0, SCAN_CONFIG, false)),
                ("scan_chunks", buffer(1, Kind::Read)),
                ("segment_ranges", buffer(2, Kind::Write)),
                ("segment_tile_cursors", buffer(3, Kind::Write)),
                ("chunk_offsets", buffer(4, Kind::Write)),
                ("active_indices", buffer(5, Kind::Read)),
                ("dispatch_grid", uniform(31, DISPATCH_GRID, true)),
            ],
            &[(
                "scan_apply_chunk_offsets",
                &[
                    "config",
                    "scan_chunks",
                    "segment_ranges",
                    "segment_tile_cursors",
                    "chunk_offsets",
                    "active_indices",
                    "dispatch_grid",
                ],
            )],
        ),
        "scan-chunk-offsets" => interface(
            [constants["SCAN_CHUNK_SIZE"], 1, 1],
            &[
                ("config", uniform(0, SCAN_CONFIG, false)),
                ("path_records", buffer(1, Kind::Read)),
                ("scan_chunk_ranges", buffer(2, Kind::Read)),
                ("segment_bumps", buffer(3, Kind::Write)),
                ("chunk_totals", buffer(4, Kind::Write)),
                ("chunk_offsets", buffer(5, Kind::Write)),
                ("active_indices", buffer(6, Kind::Read)),
            ],
            &[(
                "scan_chunk_offsets",
                &[
                    "config",
                    "path_records",
                    "scan_chunk_ranges",
                    "segment_bumps",
                    "chunk_totals",
                    "chunk_offsets",
                    "active_indices",
                ],
            )],
        ),
        "scan-clear" => interface(
            [constants["SCAN_CHUNK_SIZE"], 1, 1],
            &[
                ("config", uniform(0, SCAN_CONFIG, false)),
                ("backdrops", buffer(1, Kind::Write)),
                ("segment_ranges", buffer(2, Kind::Write)),
                ("segment_tile_counts", buffer(3, Kind::Write)),
                ("segment_tile_cursors", buffer(4, Kind::Write)),
                ("segment_bumps", buffer(5, Kind::Write)),
                ("chunk_totals", buffer(6, Kind::Write)),
                ("chunk_offsets", buffer(7, Kind::Write)),
                ("active_indices", buffer(8, Kind::Read)),
            ],
            &[(
                "scan_clear",
                &[
                    "config",
                    "backdrops",
                    "segment_ranges",
                    "segment_tile_counts",
                    "segment_tile_cursors",
                    "segment_bumps",
                    "chunk_totals",
                    "chunk_offsets",
                    "active_indices",
                ],
            )],
        ),
        "scan-count" => interface(
            [constants["SCAN_CHUNK_SIZE"], 1, 1],
            &[
                ("config", uniform(0, SCAN_CONFIG, false)),
                ("lines", buffer(1, Kind::Read)),
                ("path_records", buffer(2, Kind::Read)),
                ("backdrops", buffer(3, Kind::Write)),
                ("segment_tile_counts", buffer(4, Kind::Write)),
                ("active_indices", buffer(5, Kind::Read)),
            ],
            &[(
                "scan_count",
                &[
                    "config",
                    "lines",
                    "path_records",
                    "backdrops",
                    "segment_tile_counts",
                    "active_indices",
                ],
            )],
        ),
        "scan-emit" => interface(
            [constants["SCAN_CHUNK_SIZE"], 1, 1],
            &[
                ("config", uniform(0, SCAN_CONFIG, false)),
                ("lines", buffer(1, Kind::Read)),
                ("path_records", buffer(2, Kind::Read)),
                ("segment_tile_cursors", buffer(3, Kind::Write)),
                ("segments", buffer(4, Kind::Write)),
                ("active_indices", buffer(5, Kind::Read)),
            ],
            &[(
                "scan_emit",
                &[
                    "config",
                    "lines",
                    "path_records",
                    "segment_tile_cursors",
                    "segments",
                    "active_indices",
                ],
            )],
        ),
        "scan-prefix-chunks" => interface(
            [constants["SCAN_CHUNK_SIZE"], 1, 1],
            &[
                ("config", uniform(0, SCAN_CONFIG, false)),
                ("scan_chunks", buffer(1, Kind::Read)),
                ("segment_ranges", buffer(2, Kind::Write)),
                ("segment_tile_counts", buffer(3, Kind::Read)),
                ("chunk_totals", buffer(4, Kind::Write)),
                ("active_indices", buffer(5, Kind::Read)),
                ("dispatch_grid", uniform(31, DISPATCH_GRID, true)),
            ],
            &[(
                "scan_prefix_chunks",
                &[
                    "config",
                    "scan_chunks",
                    "segment_ranges",
                    "segment_tile_counts",
                    "chunk_totals",
                    "active_indices",
                    "dispatch_grid",
                ],
            )],
        ),
        _ => return Err(io::Error::other(format!("unknown shader family: {family}"))),
    })
}
