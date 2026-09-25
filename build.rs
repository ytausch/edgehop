fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=assets");

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        resources::embed();
    }
}

#[cfg(windows)]
mod resources {
    use std::env;
    use std::fs::{self, File};
    use std::path::PathBuf;

    use resvg::tiny_skia::{Pixmap, Transform};
    use resvg::usvg::Tree;

    // The cursor turns to mush below 32 px, so the small sizes show only the ring.
    const ICONS: [(&str, &[u32]); 2] = [
        ("assets/icon-small.svg", &[16, 20, 24]),
        ("assets/icon.svg", &[32, 40, 48, 64, 256]),
    ];

    // Without this, Windows opens a console for edgehop whenever it isn't
    // started from one, such as at login. From a terminal, edgehop still uses
    // the terminal's console. Needs Windows 11 24H2; older versions ignore it.
    const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application>
    <windowsSettings>
      <consoleAllocationPolicy xmlns="http://schemas.microsoft.com/SMI/2024/WindowsSettings">detached</consoleAllocationPolicy>
    </windowsSettings>
  </application>
</assembly>
"#;

    /// Embed the manifest, and the SVGs rendered into an .ico as the icon that
    /// Explorer and the taskbar show for edgehop.exe.
    pub fn embed() {
        let mut icon = ico::IconDir::new(ico::ResourceType::Icon);
        for (svg, sizes) in ICONS {
            let data = fs::read(svg).expect("failed to read the icon");
            let tree = Tree::from_data(&data, &Default::default()).expect("invalid icon SVG");
            for &size in sizes {
                let image = ico::IconImage::from_rgba_data(size, size, render(&tree, size));
                icon.add_entry(
                    ico::IconDirEntry::encode(&image).expect("failed to encode the icon"),
                );
            }
        }

        let path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("edgehop.ico");
        icon.write(File::create(&path).unwrap())
            .expect("failed to write the icon");
        winresource::WindowsResource::new()
            .set_icon(path.to_str().unwrap())
            .set_manifest(MANIFEST)
            .compile()
            .expect("failed to embed the resources");
    }

    fn render(tree: &Tree, size: u32) -> Vec<u8> {
        let mut pixmap = Pixmap::new(size, size).unwrap();
        let scale = size as f32 / tree.size().width();
        resvg::render(
            tree,
            Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        // tiny-skia stores premultiplied alpha; .ico wants it straight.
        pixmap
            .pixels()
            .iter()
            .flat_map(|pixel| {
                let color = pixel.demultiply();
                [color.red(), color.green(), color.blue(), color.alpha()]
            })
            .collect()
    }
}
