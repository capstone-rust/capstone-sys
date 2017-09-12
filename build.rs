//! The following environment variables affect the build:
//!
//! * `UPDATE_CAPSTONE_BINDINGS`: setting indicates that the pre-generated `capstone.rs` should be
//!   updated with the output bindgen

#[cfg(feature = "use_bindgen")]
extern crate bindgen;

#[cfg(feature = "use_system_capstone")]
extern crate pkg_config;

#[cfg(any(feature = "build_capstone_cmake", windows))]
extern crate cmake;

use std::fs::copy;
use std::path::PathBuf;
use std::process::Command;
use std::env;

#[cfg(feature = "use_bindgen")]
include!("common.rs");

/// Indicates how capstone library should be linked
#[allow(dead_code)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum LinkType {
    Dynamic,
    Static,
}

impl LinkType {
    /// File extension for libraries for the target system
    fn lib_extension(self) -> &'static str {
        let target = env::var("TARGET").unwrap();
        if target.contains("windows-msvc") {
            // Windows
            match self {
                LinkType::Dynamic => "dll",
                LinkType::Static => "lib",
            }
        } else if target.contains("apple") {
            // Mac OS
            match self {
                LinkType::Dynamic => "dylib",
                LinkType::Static => "a",
            }
        } else {
            // Unix like
            match self {
                LinkType::Dynamic => "so",
                LinkType::Static => "a",
            }
        }
    }
}

/// Build capstone with cmake
#[cfg(any(feature = "build_capstone_cmake", windows))]
fn cmake() {
    let mut cfg = cmake::Config::new("capstone");
    let dst = cfg.build();

    // The `cmake` crate builds capstone from the OUT directory automatically
    println!("cargo:rustc-link-search=native={}/lib", dst.display());
}

/// Search for header in search paths
#[cfg(feature = "use_bindgen")]
fn find_capstone_header(header_search_paths: &Vec<PathBuf>, name: &str) -> Option<PathBuf> {
    for search_path in header_search_paths.iter() {
        let potential_file = search_path.join(name);
        if potential_file.is_file() {
            return Some(potential_file);
        }
    }
    None
}

/// Create bindings using bindgen
#[cfg(feature = "use_bindgen")]
fn write_bindgen_bindings(header_search_paths: &Vec<PathBuf>, update_pregenerated_bindings: bool) {
    let mut builder = bindgen::Builder::default()
        .rust_target(bindgen::RustTarget::Stable_1_19)
        .header(
            find_capstone_header(header_search_paths, "capstone.h")
                .expect("Could not find header")
                .to_str()
                .unwrap(),
        )
        .disable_name_namespacing()
        .prepend_enum_name(false)
        .generate_comments(true)
        .constified_enum_module("[^_]+_reg$"); // Some registers have aliases


    // Whitelist cs_.* functions and types
    let pattern = String::from("cs_.*");
    builder = builder
        .whitelisted_function(pattern.clone())
        .whitelisted_type(pattern.clone());

    // Whitelist types with architectures
    for arch in ARCH_INCLUDES {
        let pattern = format!(".*(^|_){}(_|$).*", arch.cs_name);
        builder = builder.whitelisted_type(pattern);
    }

    let bindings = builder.generate().expect("Unable to generate bindings");

    // Write bindings to $OUT_DIR/bindings.rs
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join(BINDINGS_FILE);
    bindings
        .write_to_file(out_path.clone())
        .expect("Unable to write bindings");

    if update_pregenerated_bindings {
        let stored_bindgen_header: PathBuf = [
            env::var("CARGO_MANIFEST_DIR").expect("Could not find cargo environment variable"),
            "pre_generated".into(),
            BINDINGS_FILE.into(),
        ].iter()
            .collect();
        copy(out_path, stored_bindgen_header).expect("Unable to update capstone bindings");
    }
}

/// Find system capstone library and return link type
#[cfg(feature = "use_system_capstone")]
fn find_system_capstone(header_search_paths: &mut Vec<PathBuf>) -> Option<LinkType> {
    assert!(
        !cfg!(feature = "build_capstone_cmake"),
        "build_capstone_cmake feature is only valid when building bundled capstone"
    );

    let capstone_lib =
        pkg_config::find_library("capstone").expect("Could not find system capstone");
    header_search_paths.append(&mut capstone_lib.include_paths.clone());
    Some(LinkType::Dynamic)
}

fn main() {
    #[allow(unused_assignments)]
    let mut link_type: Option<LinkType> = None;

    // C header search paths
    let mut header_search_paths: Vec<PathBuf> = Vec::new();

    if cfg!(feature = "use_system_capstone") {
        #[cfg(feature = "use_system_capstone")]
        {
            link_type = find_system_capstone(&mut header_search_paths);
        }
    } else {
        link_type = Some(LinkType::Static);
        if cfg!(feature = "build_capstone_cmake") || cfg!(windows) {
            #[cfg(any(feature = "build_capstone_cmake", windows))]
            cmake();
        } else {
            let out_dir = env::var("OUT_DIR").expect("Cannot find OUT_DIR");
            Command::new("./make.sh")
                .current_dir("capstone")
                .status()
                .expect("Failed to build bundled capstone library");
            let capstone_lib = format!("libcapstone.{}", link_type.unwrap().lib_extension());
            let out_dir_dst: PathBuf = [&out_dir, &capstone_lib].iter().collect();
            let capstone_lib_path: PathBuf = [
                &env::var("CARGO_MANIFEST_DIR").unwrap(),
                "capstone",
                &capstone_lib,
            ].iter()
                .collect();
            copy(&capstone_lib_path, &out_dir_dst).expect("Failed to copy capstone lib to OUT_DIR");
            println!("cargo:rustc-link-search=native={}", out_dir);
        }
        header_search_paths.push(PathBuf::from("capstone/include"));
    }

    match link_type.expect("Must specify link type") {
        LinkType::Dynamic => {
            println!("cargo:rustc-link-lib=dylib=capstone");
        }
        LinkType::Static => {
            println!("cargo:rustc-link-lib=static=capstone");
        }
    }

    // If UPDATE_CAPSTONE_BINDINGS is set, then updated the pre-generated capstone bindings
    let update_pregenerated_bindings = env::var("UPDATE_CAPSTONE_BINDINGS").is_ok();
    if update_pregenerated_bindings {
        assert!(
            cfg!(feature = "use_bindgen"),
            concat!(
                "Setting UPDATE_CAPSTONE_BINDINGS only makes ",
                "sense when enabling feature \"use_bindgen\""
            )
        );
    }

    // Only run bindgen if we are *not* using the bundled capstone bindings
    #[cfg(feature = "use_bindgen")]
    write_bindgen_bindings(&header_search_paths, update_pregenerated_bindings);
}
