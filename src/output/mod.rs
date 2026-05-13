use anyhow::Result;

pub fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub fn eprint_error_json(e: &anyhow::Error) {
    let msg = serde_json::to_string_pretty(&serde_json::json!({"error": format!("{:#}", e)}))
        .unwrap_or_else(|_| "{\"error\":\"unknown\"}".to_string());
    eprintln!("{msg}");
}
