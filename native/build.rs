fn main() {
    println!("cargo:rerun-if-changed=res/app.rc");
    println!("cargo:rerun-if-changed=res/app.manifest");
    embed_resource::compile("res/app.rc", embed_resource::NONE);
}
