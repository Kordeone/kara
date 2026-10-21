use std::{env, fs, path::PathBuf};
use xdgen::{App, Context, FluentString};

fn main() {
    let ctx = Context::new("i18n", env::var("CARGO_PKG_NAME").unwrap()).unwrap();
    let app = App::new(FluentString("app-title"))
        .comment(FluentString("app-comment"))
        .keywords(FluentString("app-keywords"));

    let desktop_entry = app.expand_desktop("resources/app.desktop", &ctx).unwrap();
    let metainfo = app
        .expand_metainfo("resources/app.metainfo.xml", &ctx)
        .unwrap();

    println!("cargo:rerun-if-changed=resources/app.desktop");
    println!("cargo:rerun-if-changed=resources/app.metainfo.xml");
    println!("cargo:rerun-if-changed=i18n");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("xdgen");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("app.desktop"), desktop_entry).unwrap();
    fs::write(output.join("app.metainfo.xml"), metainfo).unwrap();
}
