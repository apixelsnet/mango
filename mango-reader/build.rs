//! Makes sure there is a `ui/dist` to embed, so the core builds before the UI has
//! been built. `npm run build` in `ui/` replaces the stand-in with the real thing.

use std::{env, fs, path::Path};

fn main() {
    // A release build is told its version, commit and repository; any other build
    // goes by Cargo.toml and knows neither of the last two.
    let said = |name: &str| {
        println!("cargo:rerun-if-env-changed={name}");
        env::var(name).ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty())
    };
    let version = said("MANGO_VERSION").map(|tag| tag.trim_start_matches('v').to_owned());
    println!("cargo:rustc-env=MANGO_VERSION={}", version.unwrap_or_else(|| env::var("CARGO_PKG_VERSION").expect("cargo sets the version")));
    println!("cargo:rustc-env=MANGO_COMMIT={}", said("MANGO_COMMIT").map(|commit| commit.chars().take(7).collect::<String>()).unwrap_or_default());
    println!("cargo:rustc-env=MANGO_REPO={}", said("MANGO_REPO").unwrap_or_default());

    println!("cargo:rerun-if-changed=ui/dist");
    let index = Path::new("ui/dist/index.html");
    if !index.exists() {
        fs::create_dir_all("ui/dist").expect("create ui/dist");
        fs::write(index, "<!doctype html><title>Mango</title><p>The UI has not been built. Run <code>npm run build</code> in <code>ui/</code>, then build Mango again.</p>\n")
            .expect("write the stand-in page");
    }
}
