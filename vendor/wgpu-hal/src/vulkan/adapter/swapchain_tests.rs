use super::*;

#[test]
fn deferred_swapchain_allocation_requires_an_enabled_device_feature() {
    let supported =
        vk::PhysicalDeviceSwapchainMaintenance1FeaturesEXT::default().swapchain_maintenance1(true);
    let unsupported = vk::PhysicalDeviceSwapchainMaintenance1FeaturesEXT::default();
    for (support, extensions, enabled) in [
        (
            Some(&supported),
            vec![ext::swapchain_maintenance1::NAME],
            true,
        ),
        (Some(&supported), vec![khr::swapchain::NAME], false),
        (
            Some(&unsupported),
            vec![ext::swapchain_maintenance1::NAME],
            false,
        ),
        (None, vec![ext::swapchain_maintenance1::NAME], false),
    ] {
        let mut features = PhysicalDeviceFeatures {
            core: vk::PhysicalDeviceFeatures::default().robust_buffer_access(true),
            swapchain_maintenance1: swapchain_maintenance_feature(support, &extensions),
            ..Default::default()
        };
        let info = features.add_to_device_create(vk::DeviceCreateInfo::default());
        // Check the actual device-create chain, not merely whether the driver advertises
        // the extension: using the swapchain flag without enabling this feature is invalid.
        assert_eq!(!info.p_next.is_null(), enabled);
        assert_eq!(
            unsafe { (*info.p_enabled_features).robust_buffer_access },
            vk::TRUE
        );
        if enabled {
            let feature = unsafe {
                &*info
                    .p_next
                    .cast::<vk::PhysicalDeviceSwapchainMaintenance1FeaturesEXT<'_>>()
            };
            assert_eq!(
                feature.s_type,
                vk::StructureType::PHYSICAL_DEVICE_SWAPCHAIN_MAINTENANCE_1_FEATURES_EXT
            );
            assert_eq!(feature.swapchain_maintenance1, vk::TRUE);
            assert!(feature.p_next.is_null());
        }
    }
}
