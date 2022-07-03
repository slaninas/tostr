// From https://rust-lang.github.io/rust-bindgen/tutorial-3.html

use std::env;
use std::path::PathBuf;

fn main() {
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Clone nostril repository
    std::process::Command::new("bash")
        .arg("-c")
        .arg("git clone https://github.com/jb55/nostril /nostril").status().unwrap();

    // Add -fPIC to CFLAGS
    std::process::Command::new("bash")
        .arg("-c")
        .arg(r"grep 'fPIC' /nostril/Makefile || sed -i 's/\(^CFLAGS.*\)/\1 -fPIC/' /nostril/Makefile").status().unwrap();

    // Add libnostril.so target
    std::process::Command::new("bash")
        .arg("-c")
        .arg("grep 'libnostril.so' /nostril/Makefile || echo 'libnostril.so: $(OBJS)\n\tgcc -shared -o $@ $(OBJS)  -lsecp256k1' >> /nostril/Makefile").status().unwrap();

    // Copy/append additional code for nostril
    std::process::Command::new("bash")
        .arg("-c")
        .arg("cp nostril.h /nostril/").status().unwrap();
    std::process::Command::new("bash")
        .arg("-c")
        .arg("grep 'int test' /nostril/nostril.c || cat nostril.c >> /nostril/nostril.c").status().unwrap();

    // Build libnostril.so
    std::process::Command::new("bash")
        .arg("-c")
        .arg("cd /nostril/ && make libnostril.so")
        .status()
        .unwrap();

    // Tell cargo to look for shared libraries in the specified directory
    // println!("cargo:rustc-link-search=/usr/local/lib");
    // println!("cargo:rustc-link-search=/nostril");
    println!("cargo:rustc-link-search=/nostril");
    println!("cargo:rustc-link-search=/usr/local/lib");


    // Tell cargo to tell rustc to link the system bzip2
    // shared library.
    println!("cargo:rustc-link-lib=secp256k1");
    // println!("cargo:rustc-link-lib=nostril");
    // println!("cargo:rustc-link-lib=aes");
    // println!("cargo:rustc-link-lib=sha256");
    // println!("cargo:rustc-link-lib=base64");
    println!("cargo:rustc-link-lib=nostril");


    // Tell cargo to invalidate the built crate whenever the wrapper changes
    println!("cargo:rerun-if-changed=/nostril/nostril.h");

    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let bindings = bindgen::Builder::default()
        // The input header we would like to generate
        // bindings for.
        .header("/nostril/nostril.h")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
