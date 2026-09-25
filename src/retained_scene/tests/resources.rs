use super::*;
use crate::shared::image_resource::ImageSource;

fn vector_leaf(tile_width: u32) -> (Rc<Canvas>, ImageKey) {
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
        <defs><pattern id="p" width="{tile_width}" height="4" patternUnits="userSpaceOnUse">
            <rect width="2" height="2" fill="red"/>
        </pattern></defs><rect width="16" height="16" fill="url(#p)"/>
        </svg>"##
    );
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).unwrap();
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_svg(&tree).unwrap();
    let (key, source) = canvas.scene_images.iter().next().unwrap();
    assert!(matches!(source, ImageSource::Vector(_)));
    (Rc::new(canvas), key)
}

#[test]
fn vector_resources_follow_live_retained_nodes_and_shared_references() {
    let root = RetainedNodeId::for_owner(80_000);
    let first = RetainedNodeId::for_owner(80_001);
    let second = RetainedNodeId::for_owner(80_002);
    let (source, key) = vector_leaf(4);
    let mut scene = RetainedScene::new(35, 19, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            first,
            source.clone(),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            second,
            source.clone(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    assert_eq!(materializer.canvas().scene_images.iter().count(), 1);
    let (_, ImageSource::Vector(expected)) = source.scene_images.iter().next().unwrap() else {
        panic!("materialization must retain the vector source");
    };
    {
        let materialized = materializer.canvas();
        let (_, ImageSource::Vector(actual)) = materialized.scene_images.iter().next().unwrap()
        else {
            panic!("materialized canvas must expose the vector resource");
        };
        assert!(Rc::ptr_eq(expected, actual));
    }

    scene.transaction().remove_subtree(first).commit().unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_eq!(materializer.canvas().scene_images.iter().count(), 1);
    assert!(materializer.canvas().scene_images.contains(key));

    scene.transaction().remove_subtree(second).commit().unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_eq!(materializer.canvas().scene_images.iter().count(), 0);
    assert!(!materializer.canvas().scene_images.contains(key));
    // An externally retained source is not a live dependency of the renderer.
    assert!(source.scene_images.contains(key));
}

#[test]
fn replacing_retained_vector_content_removes_only_its_previous_resource() {
    let root = RetainedNodeId::for_owner(80_010);
    let child = RetainedNodeId::for_owner(80_011);
    let (old_source, old_key) = vector_leaf(4);
    let (new_source, new_key) = vector_leaf(7);
    assert_ne!(old_key, new_key);
    let mut scene = RetainedScene::new(35, 19, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            old_source.clone(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    scene
        .transaction()
        .replace_scene(child, new_source)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(!materializer.canvas().scene_images.contains(old_key));
    assert_eq!(materializer.canvas().scene_images.iter().count(), 1);
    assert!(materializer.canvas().scene_images.contains(new_key));
    assert!(old_source.scene_images.contains(old_key));
}
