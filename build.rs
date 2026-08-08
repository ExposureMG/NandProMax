fn main() {
    println!("cargo:rerun-if-changed=extern/libxsvf/libxsvf.h");
    println!("cargo:rerun-if-changed=extern/libxsvf/play.c");
    println!("cargo:rerun-if-changed=extern/libxsvf/svf.c");
    println!("cargo:rerun-if-changed=extern/libxsvf/xsvf.c");
    println!("cargo:rerun-if-changed=extern/libxsvf/scan.c");
    println!("cargo:rerun-if-changed=extern/libxsvf/tap.c");
    println!("cargo:rerun-if-changed=extern/libxsvf/statename.c");
    println!("cargo:rerun-if-changed=extern/libxsvf/memname.c");

    cc::Build::new()
        .include("extern/libxsvf")
        .file("extern/libxsvf/play.c")
        .file("extern/libxsvf/svf.c")
        .file("extern/libxsvf/xsvf.c")
        .file("extern/libxsvf/scan.c")
        .file("extern/libxsvf/tap.c")
        .file("extern/libxsvf/statename.c")
        .file("extern/libxsvf/memname.c")
        .warnings(false)
        .compile("xsvf");

    println!("cargo:rustc-link-lib=static=xsvf");
}
