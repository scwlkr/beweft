fn main() -> anyhow::Result<()> {
    let doc = beweft::openapi::build_document();
    if std::env::args().any(|arg| arg == "--typescript") {
        print!("{}", beweft::contract_export::typescript(&doc));
    } else {
        println!("{}", serde_json::to_string_pretty(&doc)?);
    }
    Ok(())
}
