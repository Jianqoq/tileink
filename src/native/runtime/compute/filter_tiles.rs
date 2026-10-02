//! Content-owned, frame-local tile validation and read-only GPU list reuse.
use super::{ComputeBatch, ResourceId, Result};
use rustc_hash::FxHashSet;

pub(super) struct CachedFilterTiles {
    buffer: ResourceId,
    maximum: Option<u32>,
}

/// Borrows immutable input until upload; only validation can produce this proof.
/// Separating validation from upload keeps empty/error paths free of GPU resources.
pub(crate) struct ValidatedFilterTiles<'a> {
    owner: u64,
    tiles: &'a [u32],
    maximum: Option<u32>,
    buffer: Option<ResourceId>,
}

impl ComputeBatch {
    pub(crate) fn validate_filter_tiles<'a>(
        &self,
        tiles: &'a [u32],
        tile_count: u64,
    ) -> Result<ValidatedFilterTiles<'a>> {
        let (maximum, buffer) = if let Some(cached) = self.filter_tile_cache.get(tiles) {
            if cached
                .maximum
                .is_some_and(|tile| u64::from(tile) >= tile_count)
            {
                return Err("filter active tiles must be unique and within the surface".into());
            }
            (cached.maximum, Some(cached.buffer))
        } else {
            let mut unique = FxHashSet::with_capacity_and_hasher(tiles.len(), Default::default());
            let mut maximum = 0;
            for &tile in tiles {
                if u64::from(tile) >= tile_count || !unique.insert(tile) {
                    return Err("filter active tiles must be unique and within the surface".into());
                }
                maximum = maximum.max(tile);
            }
            ((!tiles.is_empty()).then_some(maximum), None)
        };
        Ok(ValidatedFilterTiles {
            owner: self.owner,
            tiles,
            maximum,
            buffer,
        })
    }

    pub(crate) fn filter_tile_buffer(
        &mut self,
        tiles: ValidatedFilterTiles<'_>,
    ) -> Result<ResourceId> {
        if tiles.owner != self.owner {
            return Err("filter tiles belong to another compute batch".into());
        }
        if let Some(buffer) = tiles.buffer {
            return Ok(buffer);
        }
        // A second proof may have uploaded this list since the first was prepared.
        if let Some(cached) = self.filter_tile_cache.get(tiles.tiles) {
            return Ok(cached.buffer);
        }
        // Empty lists need a nonempty descriptor even though dispatch never reads it.
        let bytes = if tiles.tiles.is_empty() {
            vec![0; 4]
        } else {
            tiles
                .tiles
                .iter()
                .flat_map(|tile| tile.to_le_bytes())
                .collect()
        };
        let buffer = self.buffer(bytes)?;
        // Root-cause fix shared by Metal/DX12/Vulkan: repeated filter passes used
        // to build a BTreeSet and upload the same immutable list independently.
        // Owned content keys preserve arbitrary order and detect in-place changes;
        // each reuse rechecks the maximum for the current logical surface size.
        self.filter_tile_cache.insert(
            tiles.tiles.to_vec(),
            CachedFilterTiles {
                buffer,
                maximum: tiles.maximum,
            },
        );
        Ok(buffer)
    }
}

#[cfg(test)]
#[path = "filter_tiles/tests.rs"]
mod tests;
