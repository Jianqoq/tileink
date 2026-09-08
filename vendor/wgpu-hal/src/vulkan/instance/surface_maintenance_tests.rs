use super::*;

#[test]
fn headless_instance_does_not_enable_surface_maintenance_dependencies() {
    let mut extensions = vec![khr::get_physical_device_properties2::NAME];
    let original = extensions.clone();
    add_surface_maintenance_extensions(&mut extensions, |_| true);
    assert_eq!(extensions, original);
}

#[test]
fn surface_maintenance_enables_its_complete_instance_dependency_chain() {
    for supported in [
        vec![],
        vec![khr::get_surface_capabilities2::NAME],
        vec![ext::surface_maintenance1::NAME],
        vec![
            khr::get_surface_capabilities2::NAME,
            ext::surface_maintenance1::NAME,
        ],
    ] {
        let original = vec![khr::surface::NAME, ext::swapchain_colorspace::NAME];
        let mut extensions = original.clone();
        add_surface_maintenance_extensions(&mut extensions, |name| supported.contains(&name));
        let mut expected = original;
        if supported.len() == 2 {
            expected.extend([
                khr::get_surface_capabilities2::NAME,
                ext::surface_maintenance1::NAME,
            ]);
        }
        assert_eq!(extensions, expected, "advertised extensions: {supported:?}");
    }
}
