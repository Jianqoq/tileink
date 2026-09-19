use super::Result;
use ash::vk;
pub fn select(
    capabilities: &vk::SurfaceCapabilitiesKHR,
    size: [u32; 2],
) -> Result<(vk::Extent2D, u32)> {
    let extent = if capabilities.current_extent.width == u32::MAX {
        vk::Extent2D {
            width: size[0],
            height: size[1],
        }
    } else {
        capabilities.current_extent
    };
    if size.contains(&0)
        || [extent.width, extent.height] != size
        || extent.width < capabilities.min_image_extent.width
        || extent.height < capabilities.min_image_extent.height
        || extent.width > capabilities.max_image_extent.width
        || extent.height > capabilities.max_image_extent.height
    {
        return Err("window extent is outside current Vulkan surface capabilities".into());
    }
    let maximum = if capabilities.max_image_count == 0 {
        u32::MAX
    } else {
        capabilities.max_image_count
    };
    let count = capabilities.min_image_count.max(2).min(maximum);
    Ok((extent, count))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn honors_finite_image_counts_and_surface_extent_bounds() {
        let mut caps = vk::SurfaceCapabilitiesKHR {
            min_image_count: 1,
            max_image_count: 1,
            current_extent: vk::Extent2D {
                width: u32::MAX,
                height: u32::MAX,
            },
            min_image_extent: vk::Extent2D {
                width: 2,
                height: 3,
            },
            max_image_extent: vk::Extent2D {
                width: 100,
                height: 90,
            },
            ..Default::default()
        };
        assert_eq!(select(&caps, [20, 10]).unwrap().1, 1);
        caps.max_image_count = 0;
        assert_eq!(select(&caps, [20, 10]).unwrap().1, 2);
        for size in [[0, 10], [1, 10], [20, 2], [101, 10], [20, 91]] {
            assert!(select(&caps, size).is_err());
        }
        caps.current_extent = vk::Extent2D {
            width: 20,
            height: 10,
        };
        assert!(select(&caps, [30, 10]).is_err());
        assert!(select(&caps, [20, 10]).is_ok());
    }
}
