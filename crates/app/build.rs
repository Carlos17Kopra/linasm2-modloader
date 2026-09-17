//! One job: give the Windows executable its icon.
//!
//! Windows reads an application's icon out of the binary itself, not from
//! a file beside it — there is no `.desktop` entry to point at one. An
//! `.exe` without the resource below shows the toolkit's blank default in
//! Explorer, on the desktop and in the task bar.

fn main() {
    // The icon is shared with the Linux side and with the window icon the
    // interface sets at runtime; `packaging/make-icons.py` writes all of
    // them from the same crop.
    println!("cargo:rerun-if-changed=../../packaging/lina-sm2.ico");

    // The *target*, not the host: a cross build from Linux still has to
    // embed the resource, and a native Linux build must not try. Reading
    // `cfg!(windows)` here would answer for the machine running this
    // script, which is the wrong question.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("../../packaging/lina-sm2.ico");

    // Deliberately fatal. The alternative — a warning nobody reads — ends
    // with a release shipping an iconless executable, and nothing else in
    // the build would notice. Whoever can compile for an MSVC target has
    // the Windows SDK that holds `rc.exe`; the GNU toolchain's `windres`
    // works just as well.
    resource.compile().expect("the Windows icon resource could not be compiled");
}
