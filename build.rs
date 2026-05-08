use std::path::PathBuf;

// Files that pull in SDL/Allegro headers or are platform implementations —
// exclude them; Rust stubs in src/doom/ replace their symbols.
const EXCLUDE: &[&str] = &[
    // platform implementations (we write our own)
    "doomgeneric_allegro.c",
    "doomgeneric_emscripten.c",
    "doomgeneric_linuxvt.c",
    "doomgeneric_sdl.c",
    "doomgeneric_soso.c",
    "doomgeneric_sosox.c",
    "doomgeneric_win.c",
    "doomgeneric_xlib.c",
    // directly include SDL/Allegro headers
    "i_sound.c",
    "i_system.c",
    "i_sdlsound.c",
    "i_sdlmusic.c",
    "i_allegromusic.c",
    "i_allegrosound.c",
];

fn main() {
    if std::env::var("CARGO_FEATURE_DOOM").is_err() {
        return;
    }

    let doom_src = PathBuf::from("vendor/doomgeneric/doomgeneric");

    let sources: Vec<PathBuf> = std::fs::read_dir(&doom_src)
        .expect("vendor/doomgeneric/doomgeneric not found — run: git clone https://github.com/ozkl/doomgeneric vendor/doomgeneric")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().map(|e| e == "c").unwrap_or(false)
                && !EXCLUDE.contains(&p.file_name().unwrap().to_str().unwrap())
        })
        .collect();

    cc::Build::new()
        .file("src/ffi/stubs.c")
        .files(&sources)
        .include(&doom_src)
        // Freestanding kernel target — no hosted libc assumptions.
        .flag("-ffreestanding")
        .flag("-fno-stack-protector")
        // Disable red zone: x86_64 ABI reserves 128 bytes below RSP for
        // leaf functions, but kernel interrupt handlers will clobber it.
        .flag("-mno-red-zone")
        .flag("-fno-pic")
        .flag("-fno-builtin")
        .flag("-nostdlib")
        .flag("-m64")
        // Glibc fortify redirects (e.g. __fread_chk) are introduced at -O1+;
        // disable since we're freestanding with no libc.
        .define("_FORTIFY_SOURCE", "0")
        // Mode 13h: 320x200 256-color palette. CMAP256 makes pixel_t = uint8_t
        // so DG_ScreenBuffer holds raw palette indices we can memcpy to 0xA0000.
        .define("DOOMGENERIC_RESX", "320")
        .define("DOOMGENERIC_RESY", "200")
        .define("CMAP256", None)
        .opt_level(1)
        .warnings(false) // doom source has many; suppress noise
        .compile("doom");

    println!("cargo:rerun-if-changed=vendor/doomgeneric/doomgeneric");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/ffi/stubs.c");
}
