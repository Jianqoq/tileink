use super::*;

#[test]
fn invalid_window_geometry_errors_are_reported() {
    let window = Window {
        hwnd: HWND::default(),
        instance: HINSTANCE::default(),
    };
    assert!(window.inner_size().is_err());
    assert!(
        window
            .request_inner_size(Size {
                width: 640,
                height: 360
            })
            .is_err()
    );
}

#[test]
fn unrepresentable_window_extents_are_rejected() {
    let window = Window {
        hwnd: HWND::default(),
        instance: HINSTANCE::default(),
    };
    for size in [
        Size {
            width: u32::MAX,
            height: 360,
        },
        Size {
            width: 640,
            height: u32::MAX,
        },
        Size {
            width: i32::MAX as u32,
            height: 360,
        },
        Size {
            width: 640,
            height: i32::MAX as u32,
        },
    ] {
        let error = window.request_inner_size(size).unwrap_err();
        assert!(error.is::<std::num::TryFromIntError>());
    }
}

#[test]
fn hidden_window_reports_requested_client_sizes() {
    let window = Window::new("Tileink geometry test", false).unwrap();
    for size in [
        Size {
            width: 640,
            height: 360,
        },
        Size {
            width: 480,
            height: 270,
        },
    ] {
        window.request_inner_size(size).unwrap();
        let actual = window.inner_size().unwrap();
        assert_eq!((actual.width, actual.height), (size.width, size.height));
    }
}
