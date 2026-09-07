//! Seed a fresh debug-only profile for the CLI/MCP Windows smoke run.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let profile = kw_storage::paths::debug_profile()
        .ok_or("set KW_AUTOMATION_PROFILE to an absolute, fresh directory (debug builds only)")?;
    let path = profile.join("knegiswhisper.db");
    if path.exists() {
        return Err("refusing to overwrite an existing profile".into());
    }
    let db = kw_storage::Db::open(&path).await?;
    kw_storage::settings::set(db.pool(), "localApi.enabled", &true).await?;
    kw_storage::settings::set(
        db.pool(),
        "transcription.route",
        &serde_json::json!({"useLocalWhisper":true}),
    )
    .await?;
    kw_storage::settings::set(db.pool(), "transcription.localModel", &"tiny").await?;
    kw_storage::settings::set(db.pool(), "meetings.promptOnDetect", &false).await?;
    db.close().await;
    println!(
        "Created isolated local-only smoke profile at {}",
        profile.display()
    );
    Ok(())
}
