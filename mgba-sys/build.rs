extern crate bindgen;

use std::env;
use std::io::BufRead;
use std::path::{Path, PathBuf};

/// Pull the `-D…` flags cmake fed to the mgba C compile out of whatever
/// per-generator layout cmake produced this time:
///
/// * Makefile-style generators (NMake / Unix Makefiles / MinGW Makefiles)
///   write `build/CMakeFiles/mgba.dir/flags.make` with a literal
///   `C_DEFINES = -Dfoo -Dbar` line.
/// * Visual Studio generators write `build/mgba.vcxproj` (XML) with one
///   `<PreprocessorDefinitions>FOO;BAR;%(PreprocessorDefinitions)</…>`
///   element per build config. We grab the first non-empty one — the
///   defines don't differ meaningfully across Debug/Release for the
///   bindgen-visible header set.
fn extract_c_defines(build_dir: &Path) -> Option<Vec<String>> {
    let flags_make = build_dir.join("CMakeFiles").join("mgba.dir").join("flags.make");
    if flags_make.exists() {
        return extract_from_flags_make(&flags_make);
    }
    let vcxproj = build_dir.join("mgba.vcxproj");
    if vcxproj.exists() {
        return extract_from_vcxproj(&vcxproj);
    }
    None
}

fn extract_from_flags_make(path: &Path) -> Option<Vec<String>> {
    let file = std::fs::File::open(path).ok()?;
    let mut flags = None;
    for line in std::io::BufReader::new(file).lines() {
        let line = line.ok()?;
        if let Some(rest) = line.strip_prefix("C_DEFINES = ") {
            flags = Some(shell_words::split(rest).ok()?);
        }
    }
    flags
}

fn extract_from_vcxproj(path: &Path) -> Option<Vec<String>> {
    // The vcxproj is one big XML blob; rather than dragging in a full
    // parser, slice between the first non-empty <PreprocessorDefinitions>
    // open/close tag pair. Configs share the same defines for mgba so
    // first-match is fine.
    let content = std::fs::read_to_string(path).ok()?;
    const OPEN: &str = "<PreprocessorDefinitions>";
    const CLOSE: &str = "</PreprocessorDefinitions>";
    let mut cursor = 0;
    while let Some(start) = content[cursor..].find(OPEN) {
        let abs_start = cursor + start + OPEN.len();
        let end = content[abs_start..].find(CLOSE)?;
        let raw = &content[abs_start..abs_start + end];
        let flags: Vec<String> = raw
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with("%("))
            .map(|s| format!("-D{s}"))
            .collect();
        if !flags.is_empty() {
            return Some(flags);
        }
        cursor = abs_start + end + CLOSE.len();
    }
    None
}

/// Extra preprocessor defines forced onto the mgba build.
///
/// `COLOR_16_BIT` switches mgba's `mColor` from 32-bit XBGR8 to the GBA-native
/// 15-bit BGR555 (no `COLOR_5_6_5`, so it stays BGR5, not RGB565), letting
/// tango do its own color conversion off the raw framebuffer.
///
/// `MINIMAL_CORE=1` drops the frontend machinery from the core — the input
/// map, video logger, proxy renderers, and the Dolphin SIO driver — and with
/// it fields of `struct mCore` itself (`inputMap`, the `startVideoLog`/
/// `endVideoLog` vtable slots), so it carries the same discipline. Note the
/// macro only reaches the C compile through this `-D` flag: the cmake
/// `MINIMAL_CORE` variable gates the source lists without ever defining the
/// macro (the generated flags.h is read by nothing but mgba's python
/// bindings), and a variable-only build leaves the core objects with dangling
/// references to the sources it pruned. Level 1, not the libretro level 2 —
/// level 2 also severs the config override wiring that cart detection
/// (RTC/savedata) feeds on.
///
/// `DISABLE_THREADING` compiles out mgba's own thread runner (mCoreThread),
/// the video thread proxy, and threaded rewind. Threading lives on the Rust
/// side: cores are driven on threads we own, so all of this is dead code —
/// and MINIMAL_CORE all but demands it, since every upstream MINIMAL_CORE
/// configuration pairs the two (GBACoreInit unconditionally instantiates the
/// video thread proxy whenever threading is on, from sources MINIMAL_CORE
/// prunes). It also gates fields of public structs (mCoreRewindContext), so
/// bindgen must see it for the same layout reasons as the rest.
///
/// These must reach BOTH the C compile (via cmake CFLAGS) and the bindgen pass
/// (via clang args) — if only one side sees them, `mColor`'s width disagrees
/// across the FFI boundary and the video buffer is silently misinterpreted.
const FORCED_DEFINES: &[&str] = &["COLOR_16_BIT", "MINIMAL_CORE=1", "DISABLE_THREADING"];

/// Locate the wasi-sdk root: `WASI_SDK_PATH`, required for wasm32 builds.
fn wasi_sdk() -> PathBuf {
    PathBuf::from(
        env::var("WASI_SDK_PATH").expect("set WASI_SDK_PATH to a wasi-sdk root to build mgba for wasm32"),
    )
}

/// wasi-sdk's compiler-rt builtins archive for wasm32 — the layout moved
/// across sdk releases, so glob for it instead of hardcoding a clang
/// version.
fn wasm32_builtins_dir(wasi_sdk: &Path) -> Option<PathBuf> {
    let clang_lib = wasi_sdk.join("lib").join("clang");
    for version in std::fs::read_dir(clang_lib).ok()? {
        let lib = version.ok()?.path().join("lib");
        for flavor in std::fs::read_dir(lib).ok()? {
            let dir = flavor.ok()?.path();
            if dir.join("libclang_rt.builtins-wasm32.a").exists() {
                return Some(dir);
            }
        }
    }
    None
}

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let wasm = target_arch == "wasm32";

    let mut cfg = cmake::Config::new("mgba");
    cfg.define("LIBMGBA_ONLY", "on");
    // Prune the SIO/extra/feature source lists to match the MINIMAL_CORE
    // define — without this the pruned code still compiles, just dead.
    cfg.define("MINIMAL_CORE", "ON");
    // Threading is compiled out (DISABLE_THREADING above); keep cmake from
    // probing for pthreads and feeding the compile -pthread/USE_PTHREADS.
    cfg.define("USE_PTHREADS", "OFF");
    for def in FORCED_DEFINES {
        cfg.cflag(format!("-D{def}"));
    }
    if wasm {
        let sdk = wasi_sdk();
        // The toolchain file owns compiler/sysroot/system-name; keep the
        // cmake crate's host-derived compiler flags out of the build.
        cfg.define(
            "CMAKE_TOOLCHAIN_FILE",
            sdk.join("share").join("cmake").join("wasi-sdk.cmake"),
        );
        cfg.no_default_flags(true);
        // Drop assert() so the core doesn't pull fprintf/abort paths
        // beyond what the shim covers.
        cfg.cflag("-DNDEBUG");
        // The UNIX branch normally supplies _GNU_SOURCE; without it
        // wasi-libc's headers hide the BSD/GNU declarations mgba's
        // feature checks find in the library (strlcpy et al).
        cfg.cflag("-D_GNU_SOURCE");
        // wasi-libc deliberately leaves PATH_MAX undefined (WASI paths
        // have no fixed bound); mgba only uses it for config-dir string
        // buffers that are dead code under the in-memory VFS.
        cfg.cflag("-DPATH_MAX=4096");
        // core/thread.c includes <signal.h> outside its DISABLE_THREADING
        // guard. The emulation define only unlocks the declarations —
        // with the thread body compiled out nothing references a signal
        // function, so there is no -lwasi-emulated-signal to link.
        cfg.cflag("-D_WASI_EMULATED_SIGNAL");
        // vfs.c demands SOME path-open backend even though everything we
        // load goes through the in-memory Rust VFS. The stdio backend is
        // the only one WASI can express; its vfs-file.c isn't in the
        // platform source list (no WASI branch in CMakeLists), so it is
        // compiled into the shim archive below instead.
        cfg.cflag("-DENABLE_VFS_FILE");
    }

    let mgba_dst = cfg.build();
    let build_dir = mgba_dst.join("build");
    let flags = extract_c_defines(&build_dir).expect("could not extract C_DEFINES from cmake build");

    // MINIMAL_CORE drops the gba/sio driver sources from the cmake source
    // list (they ride in the same list as the Dolphin driver), but the
    // lockstep and wireless drivers are exactly the pieces the rollback
    // stack drives the cores through. Compile them back in with the same
    // defines cmake fed the core objects so struct layouts agree. Emitted
    // before libmgba so single-pass linkers resolve their references into
    // the core archive. On wasm they ride the shim archive below instead —
    // same idea, wasi compiler.
    if !wasm {
        let mut sio = cc::Build::new();
        sio.include("mgba/include").include(build_dir.join("include"));
        for def in FORCED_DEFINES {
            sio.flag(format!("-D{def}"));
        }
        for flag in &flags {
            sio.flag(flag);
        }
        sio.file("mgba/src/gba/sio/lockstep.c")
            .file("mgba/src/gba/sio/wireless.c")
            .compile("mgba_sio");
    }
    println!("cargo:rerun-if-changed=mgba/src/gba/sio/lockstep.c");
    println!("cargo:rerun-if-changed=mgba/src/gba/sio/wireless.c");

    // Makefile generators (NMake / Unix / MinGW) output directly under
    // `build/`; the Visual Studio generator buries artifacts in a
    // per-config subdir (`build/Release/` for cargo release builds).
    // Emit both so cargo's link-search picks up whichever actually
    // contains `mgba.lib` / `libmgba.a`.
    println!("cargo:rustc-link-search=native={}", build_dir.display());
    for config in ["Release", "Debug", "MinSizeRel", "RelWithDebInfo"] {
        println!("cargo:rustc-link-search=native={}/{}", build_dir.display(), config);
    }
    println!("cargo:rustc-link-lib=static=mgba");

    if wasm {
        let sdk = wasi_sdk();
        // The shim archive: syscall shims, plus the two C files the
        // platform source lists leave out under WASI (the stdio VFS
        // backend and the lockstep SIO driver). Compiled with the SAME
        // defines cmake fed the core objects, so struct layouts agree.
        // Its symbols must resolve BEFORE wasi-libc so the libc objects
        // that import wasi_snapshot_preview1 are never pulled in.
        let mut shim = cc::Build::new();
        shim.compiler(sdk.join("bin").join("clang"))
            .target("wasm32-wasip1")
            .flag(format!("--sysroot={}", sdk.join("share").join("wasi-sysroot").display()))
            .include("mgba/include")
            .include(build_dir.join("include"))
            .define("_GNU_SOURCE", None)
            .define("PATH_MAX", "4096")
            .define("_WASI_EMULATED_SIGNAL", None)
            .define("NDEBUG", None)
            .define("ENABLE_VFS_FILE", None);
        for def in FORCED_DEFINES {
            shim.flag(format!("-D{def}"));
        }
        for flag in &flags {
            shim.flag(flag);
        }
        shim.file("shim.c")
            .file("wasi-stubs.c")
            .file("mgba/src/util/vfs/vfs-file.c")
            .file("mgba/src/gba/sio/lockstep.c")
            .file("mgba/src/gba/sio/wireless.c")
            .compile("mgba_wasm_shim");
        println!(
            "cargo:rustc-link-search=native={}",
            sdk.join("share").join("wasi-sysroot").join("lib").join("wasm32-wasip1").display()
        );
        println!("cargo:rustc-link-lib=static=c");
        // Compiler builtins for the C objects (i64/i128/float helpers
        // Rust's own compiler-builtins may not export).
        if let Some(dir) = wasm32_builtins_dir(&sdk) {
            println!("cargo:rustc-link-search=native={}", dir.display());
            println!("cargo:rustc-link-lib=static=clang_rt.builtins-wasm32");
        }
        println!("cargo:rerun-if-changed=shim.c");
        println!("cargo:rerun-if-changed=wasi-stubs.c");
        println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");
    } else {
        match target_os.as_str() {
            "macos" => {
                println!("cargo:rustc-link-lib=framework=Cocoa");
            }
            "windows" => {
                println!("cargo:rustc-link-lib=shlwapi");
                println!("cargo:rustc-link-lib=ole32");
                println!("cargo:rustc-link-lib=uuid");
            }
            "linux" => {}
            tos => panic!("unknown target os {:?}!", tos),
        }
    }
    println!("cargo:rerun-if-changed=wrapper.h");
    // We emit explicit rerun-if-changed directives, which override cargo's
    // default of re-running on any package change — so track build.rs itself,
    // or edits to FORCED_DEFINES (e.g. toggling COLOR_16_BIT) won't take effect.
    println!("cargo:rerun-if-changed=build.rs");

    let mut builder = bindgen::Builder::default();
    if wasm {
        // Parse the headers exactly as the wasi compile saw them: ILP32
        // layouts and wasi-libc's headers, not the host's. Needs a
        // wasm-aware libclang (e.g. LIBCLANG_PATH=$(brew --prefix llvm)/lib).
        let sdk = wasi_sdk();
        builder = builder.clang_args([
            "--target=wasm32-wasip1".to_string(),
            format!("--sysroot={}", sdk.join("share").join("wasi-sysroot").display()),
            // clang defaults wasm symbols to hidden visibility, and
            // bindgen silently drops every function it considers
            // non-linkable — this restores them.
            "-fvisibility=default".to_string(),
        ]);
    }
    let bindings = builder
        .header("wrapper.h")
        .blocklist_item("FP_INFINITE")
        .blocklist_item("FP_NAN")
        .blocklist_item("FP_NORMAL")
        .blocklist_item("FP_SUBNORMAL")
        .blocklist_item("FP_ZERO")
        .blocklist_item("FP_INT_UPWARD")
        .blocklist_item("FP_INT_DOWNWARD")
        .blocklist_item("FP_INT_TOWARDZERO")
        .blocklist_item("FP_INT_TONEARESTFROMZERO")
        .blocklist_item("FP_INT_TONEAREST")
        .blocklist_item("IPPORT_RESERVED")
        .clang_args(&["-Imgba/include", "-D__STDC_NO_THREADS__=1"])
        .clang_args(FORCED_DEFINES.iter().map(|def| format!("-D{def}")))
        .clang_args(flags)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    if wasm {
        // wasi-libc's stdio.h #undefs and redefines the SEEK_* whence
        // macros (they also live in unistd.h), and bindgen drops macros
        // that were ever #undef'd. The values are fixed by POSIX and by
        // the native bindings; append them verbatim.
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(out_path.join("bindings.rs"))
            .expect("bindings.rs vanished");
        writeln!(
            f,
            "pub const SEEK_SET: u32 = 0;\npub const SEEK_CUR: u32 = 1;\npub const SEEK_END: u32 = 2;"
        )
        .expect("couldn't append SEEK_* constants");
    }
}
