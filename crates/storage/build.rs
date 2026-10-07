fn main() {
    // sqlx::migrate! embeds a directory. Track additions as well as edits so
    // incremental builds cannot retain the previous migration inventory.
    println!("cargo:rerun-if-changed=migrations");
}
