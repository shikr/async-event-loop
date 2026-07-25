use rquickjs::Result;
use runtime::Runtime;

mod runtime;

fn main() -> Result<()> {
    Runtime::new_with_file(format!("{}/{}", env!("CARGO_MANIFEST_DIR"), "main.js"))?.run()?;

    Ok(())
}
