fn main() {
    println!("cargo:rerun-if-changed=installer/app.ico");
    println!("cargo:rerun-if-changed=app.rc");
    embed_resource::compile("app.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
