use calling::camera::list_cameras;
use calling::screen::{ShareKind, list_share_sources};

fn main() {
    let cameras = list_cameras();
    println!("cameras: {}", cameras.len());
    for (index, camera) in cameras.iter().enumerate() {
        println!("  camera {index}: key {:?}, name {} chars", camera.key, camera.name.chars().count());
    }
    let sources = list_share_sources();
    for kind in [ShareKind::Screen, ShareKind::Window] {
        let of_kind: Vec<_> = sources.iter().filter(|source| source.kind == kind).collect();
        println!("{kind:?} sources: {}", of_kind.len());
        for (index, source) in of_kind.iter().enumerate() {
            println!("  {kind:?} {index}: id {}, title {} chars", source.id, source.title.chars().count());
        }
    }
}
