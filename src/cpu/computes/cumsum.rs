use crate::shared::bd_record::BackdropRecord;

pub(crate) fn run_backdrop_cumsum(backdrops: &mut [i32], records: &[BackdropRecord]) {
    for record in records {
        let stride = (record.tile_x1 - record.tile_x0) as usize;
        let height = (record.tile_y1 - record.tile_y0) as usize;
        if stride == 0 || height == 0 {
            continue;
        }

        let base = record.data_offset as usize;
        let len = record.data_len as usize;
        let path_backdrop = &mut backdrops[base..base + len];

        for row in 0..height {
            let row_base = row * stride;
            let row_slice = &mut path_backdrop[row_base..row_base + stride];
            let mut carry = 0;
            for value in row_slice {
                carry += *value;
                *value = carry;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::run_backdrop_cumsum;
    use crate::shared::bd_record::BackdropRecord;

    #[test]
    fn scans_each_backdrop_row_independently() {
        let record = BackdropRecord {
            data_offset: 0,
            data_len: 6,
            tile_x0: 10,
            tile_y0: 20,
            tile_x1: 13,
            tile_y1: 22,
            ..BackdropRecord::default()
        };
        let mut backdrops = vec![1, -1, 2, 3, 0, -2];

        run_backdrop_cumsum(&mut backdrops, &[record]);

        assert_eq!(backdrops, vec![1, 0, 2, 3, 3, 1]);
    }

    #[test]
    fn respects_record_offsets() {
        let records = [
            BackdropRecord {
                data_offset: 1,
                data_len: 4,
                tile_x0: 0,
                tile_y0: 0,
                tile_x1: 2,
                tile_y1: 2,
                ..BackdropRecord::default()
            },
            BackdropRecord {
                data_offset: 5,
                data_len: 3,
                tile_x0: 0,
                tile_y0: 0,
                tile_x1: 3,
                tile_y1: 1,
                ..BackdropRecord::default()
            },
        ];
        let mut backdrops = vec![99, 1, 2, 3, 4, -1, 0, 2];

        run_backdrop_cumsum(&mut backdrops, &records);

        assert_eq!(backdrops, vec![99, 1, 3, 3, 7, -1, -1, 1]);
    }
}
